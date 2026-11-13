//! Writing Rich Text Format, in the dialect the original wrote and every word
//! processor since reads.

use quill_core::document::SavedView;
use quill_core::para::{Mark, Paragraph, chars};
use quill_core::props::{
    Align, BorderLine, BorderStyle, CharProps, LineSpacing, ParaProps, Rgb, RowHeight, RowProps,
    TabAlign, TabLeader, Underline, VMerge,
};
use quill_core::{Document, StoryId};
use std::fmt::Write as _;

/// Text with RTF's special characters escaped; anything outside ASCII as
/// `\uN?` so any reader gets the right character.
fn text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            chars::TAB => out.push_str("\\tab "),
            chars::LINE_BREAK => out.push_str("\\line "),
            chars::PAGE_BREAK => out.push_str("\\page "),
            chars::NONBREAKING_SPACE => out.push_str("\\~"),
            chars::OPTIONAL_HYPHEN => out.push_str("\\-"),
            chars::NONBREAKING_HYPHEN => out.push_str("\\_"),
            chars::FIELD_PAGE => out.push_str("\\chpgn "),
            chars::FIELD_DATE => out.push_str("\\chdate "),
            chars::FIELD_TIME => out.push_str("\\chtime "),
            chars::OBJECT => {}
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            c if c.is_ascii_control() => {}
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{}?", *unit as i16);
                }
            }
        }
    }
}

/// The tables runs refer to by number: colors, and the authors of
/// tracked changes (`\\revtbl`, where 0 is "Unknown").
struct Tables {
    colors: Vec<Rgb>,
    authors: Vec<String>,
    /// Where each comment's text starts and ends in the main text.
    comments: crate::docx::write::CommentEdges,
}

impl Tables {
    /// `{\\*\\atrfstart N}` or `{\\*\\atrfend N}` for the comments whose
    /// text starts or ends at `at` (paragraph, offset) of the main text.
    fn comment_marks(&self, out: &mut String, at: Option<(usize, usize)>, start: bool) {
        let Some(at) = at else {
            return;
        };
        let map = if start {
            &self.comments.starts
        } else {
            &self.comments.ends
        };
        let word = if start { "atrfstart" } else { "atrfend" };
        for id in map.get(&at).into_iter().flatten() {
            let _ = write!(out, "{{\\*\\{word} {id}}}");
        }
    }
}

impl Tables {
    /// Index in the color table; 0 is "auto".
    fn index(&self, c: Rgb) -> usize {
        self.colors
            .iter()
            .position(|&x| x == c)
            .map_or(0, |i| i + 1)
    }

    /// Index in the revision table.
    fn author(&self, name: &str) -> usize {
        self.authors.iter().position(|a| a == name).unwrap_or(0)
    }
}

fn chp(out: &mut String, c: &CharProps, tables: &Tables) {
    character_values(
        out,
        c,
        quill_core::character_styles::CharacterFormat::ALL,
        tables,
    );
    revision_words(out, c, tables);
}

fn character_values(out: &mut String, c: &CharProps, mask: u16, tables: &Tables) {
    if mask & 1 != 0 {
        let _ = write!(out, "\\f{}", c.font);
    }
    if mask & 2 != 0 {
        let _ = write!(out, "\\fs{}", c.size);
    }
    for (on, word, bit) in [
        (c.bold, "\\b", 2),
        (c.italic, "\\i", 3),
        (c.strike, "\\strike", 5),
        (c.outline, "\\outl", 6),
        (c.shadow, "\\shad", 7),
        (c.small_caps, "\\scaps", 8),
        (c.all_caps, "\\caps", 9),
        (c.hidden, "\\v", 10),
    ] {
        if mask & (1 << bit) != 0 {
            out.push_str(word);
            if !on {
                out.push('0');
            }
        }
    }
    if mask & (1 << 4) != 0 {
        out.push_str(match c.underline {
            Underline::None => "\\ulnone",
            Underline::Single => "\\ul",
            Underline::Word => "\\ulw",
            Underline::Double => "\\uldb",
            Underline::Dotted => "\\uld",
        });
    }
    if mask & (1 << 11) != 0 {
        let _ = write!(out, "\\cf{}", tables.index(c.color));
    }
    if mask & (1 << 12) != 0 {
        let _ = write!(out, "\\up{}", crate::half_points(c.position));
    }
    if mask & (1 << 13) != 0 {
        let _ = write!(out, "\\expndtw{}", c.spacing);
    }
    if mask & (1 << 14) != 0 {
        out.push_str(script_word(c.script));
    }
}

/// RTF's word for a superscript, subscript or neither.
fn script_word(script: quill_core::Script) -> &'static str {
    match script {
        quill_core::Script::None => "\\nosupersub",
        quill_core::Script::Super => "\\super",
        quill_core::Script::Sub => "\\sub",
    }
}

fn run_chp(out: &mut String, doc: &Document, _p: &Paragraph, c: &CharProps, tables: &Tables) {
    if doc.styles.character(c.character_style).is_none() {
        chp(out, c, tables);
        return;
    }
    let _ = write!(
        out,
        "\\cs{}",
        doc.styles.styles.len() + c.character_style as usize
    );
    // RTF readers may treat the preceding plain reset as direct formatting.
    // Write the resolved appearance as well as the named style, then retain
    // Quill's direct-field mask separately for editable round trips.
    character_values(
        out,
        c,
        quill_core::character_styles::CharacterFormat::ALL,
        tables,
    );
    let _ = write!(out, "\\quillchardirect{}", c.direct_format);
    revision_words(out, c, tables);
}

fn revision_words(out: &mut String, c: &CharProps, tables: &Tables) {
    if let Some(r) = quill_core::revision::get(c.rev) {
        if let Some(by) = &r.inserted {
            let _ = write!(
                out,
                "\\revised\\revauth{}\\revdttm{}",
                tables.author(&by.author),
                dttm(&by.date)
            );
        }
        if let Some(by) = &r.deleted {
            let _ = write!(
                out,
                "\\deleted\\revauthdel{}\\revdttmdel{}",
                tables.author(&by.author),
                dttm(&by.date)
            );
        }
        if let Some((by, old)) = &r.formatted {
            // Word's form: the earlier format's values over the current
            // ones, each given outright, then who changed it.
            let (auth, date) = (tables.author(&by.author), dttm(&by.date));
            let _ = write!(out, "\\crauth{auth}\\crdate{date} {{\\*\\oldcprops ");
            explicit_chp(out, old, tables);
            let _ = write!(out, "\\crauth{auth}\\crdate{date} }}");
            // After a group, a space would be text.
            return;
        }
    }
    out.push(' ');
}

/// Every character format value written outright, off ones included,
/// so it reads the same over any format.
fn explicit_chp(out: &mut String, c: &CharProps, tables: &Tables) {
    let _ = write!(out, "\\f{}\\fs{}", c.font, c.size);
    for (on, word) in [
        (c.bold, "\\b"),
        (c.italic, "\\i"),
        (c.strike, "\\strike"),
        (c.outline, "\\outl"),
        (c.shadow, "\\shad"),
        (c.small_caps, "\\scaps"),
        (c.all_caps, "\\caps"),
        (c.hidden, "\\v"),
    ] {
        out.push_str(word);
        if !on {
            out.push('0');
        }
    }
    out.push_str(match c.underline {
        Underline::None => "\\ulnone",
        Underline::Single => "\\ul",
        Underline::Word => "\\ulw",
        Underline::Double => "\\uldb",
        Underline::Dotted => "\\uld",
    });
    let _ = write!(out, "\\cf{}", tables.index(c.color));
    let shift = crate::half_points(c.position);
    if shift < 0 {
        let _ = write!(out, "\\dn{}", -shift);
    } else {
        let _ = write!(out, "\\up{shift}");
    }
    if c.script != quill_core::Script::None {
        out.push_str(script_word(c.script));
    }
    let _ = write!(out, "\\expndtw{}", c.spacing);
}

/// A tracked change's date in RTF's packed form: minutes, hours, day,
/// month, years since 1900 and weekday in one number. 0 when there is no
/// date.
pub(crate) fn dttm(date: &str) -> i32 {
    let num = |r: std::ops::Range<usize>| date.get(r).and_then(|s| s.parse::<i64>().ok());
    let (Some(y), Some(mo), Some(d)) = (num(0..4), num(5..7), num(8..10)) else {
        return 0;
    };
    let (h, mi) = (num(11..13).unwrap_or(0), num(14..16).unwrap_or(0));
    if !(1900..=2411).contains(&y) || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return 0;
    }
    let weekday = (quill_core::revision::days_from_civil(y, mo as u32, d as u32) + 4).rem_euclid(7);
    let packed = mi.clamp(0, 59)
        | h.clamp(0, 23) << 6
        | d << 11
        | mo << 16
        | (y - 1900) << 20
        | weekday << 29;
    packed as u32 as i32
}

fn border(out: &mut String, side: &str, b: &BorderLine, tables: &Tables) {
    if b.style != BorderStyle::None {
        let _ = write!(out, "\\brdr{side}");
        border_words(out, b, tables);
    }
}

/// A line's style, width, space and color.
fn border_words(out: &mut String, b: &BorderLine, tables: &Tables) {
    let (kind, width) = match b.style {
        BorderStyle::None => return,
        BorderStyle::Dotted => ("\\brdrdot", 15),
        BorderStyle::Hairline => ("\\brdrhair", 0),
        BorderStyle::Single => ("\\brdrs", 15),
        BorderStyle::Thick => ("\\brdrth", 15),
        BorderStyle::Double => ("\\brdrdb", 15),
    };
    let _ = write!(out, "{kind}\\brdrw{width}\\brsp{}", b.space);
    let color = tables.index(b.color);
    if color > 0 {
        let _ = write!(out, "\\brdrcf{color}");
    }
}

/// A frame as RTF's absolute position words.
fn frame(out: &mut String, f: &quill_core::Frame) {
    use quill_core::{FrameH, FrameRel, FrameV};
    if let Some(w) = f.width {
        let _ = write!(out, "\\absw{w}");
    }
    out.push_str(match f.h_rel {
        FrameRel::Column => "\\phcol",
        FrameRel::Margin => "\\phmrg",
        FrameRel::Page => "\\phpg",
    });
    let _ = match f.h {
        FrameH::Left => write!(out, "\\posxl"),
        FrameH::Center => write!(out, "\\posxc"),
        FrameH::Right => write!(out, "\\posxr"),
        FrameH::Inside => write!(out, "\\posxi"),
        FrameH::Outside => write!(out, "\\posxo"),
        FrameH::At(t) => write!(out, "\\posx{t}"),
    };
    out.push_str(match f.v_rel {
        FrameRel::Column => "\\pvpara",
        FrameRel::Margin => "\\pvmrg",
        FrameRel::Page => "\\pvpg",
    });
    let _ = match f.v {
        FrameV::InLine => write!(out, "\\posyil"),
        FrameV::Top => write!(out, "\\posyt"),
        FrameV::Center => write!(out, "\\posyc"),
        FrameV::Bottom => write!(out, "\\posyb"),
        FrameV::At(t) => write!(out, "\\posy{t}"),
    };
    let _ = write!(out, "\\dxfrtext{}", f.from_text);
    if f.drop_lines > 0 {
        let t = if f.in_margin { 2 } else { 1 };
        let _ = write!(out, "\\dropcapli{}\\dropcapt{t}", f.drop_lines);
    }
}

fn pap(out: &mut String, p: &ParaProps, tables: &Tables) {
    let _ = write!(out, "\\s{}", p.style);
    if p.in_table {
        out.push_str("\\intbl");
        if p.nest > 0 {
            let _ = write!(out, "\\itap{}", p.nest + 1);
        }
    }
    if let Some(f) = &p.frame {
        frame(out, f);
    }
    out.push_str(match p.align {
        Align::Left => "\\ql",
        Align::Center => "\\qc",
        Align::Right => "\\qr",
        Align::Justify => "\\qj",
    });
    let _ = write!(
        out,
        "\\li{}\\ri{}\\fi{}\\sb{}\\sa{}",
        p.left, p.right, p.first, p.before, p.after
    );
    match p.line {
        LineSpacing::Auto => {}
        LineSpacing::AtLeast(t) => {
            let _ = write!(out, "\\sl{t}\\slmult0");
        }
        LineSpacing::Exactly(t) => {
            let _ = write!(out, "\\sl-{t}\\slmult0");
        }
        LineSpacing::Multiple(m) => {
            let _ = write!(out, "\\sl{m}\\slmult1");
        }
    }
    for t in &p.tabs {
        out.push_str(match t.leader {
            TabLeader::None => "",
            TabLeader::Dots => "\\tldot",
            TabLeader::Dashes => "\\tlhyph",
            TabLeader::Underline => "\\tlul",
        });
        match t.align {
            TabAlign::Bar => {
                let _ = write!(out, "\\tb{}", t.pos);
                continue;
            }
            TabAlign::Center => out.push_str("\\tqc"),
            TabAlign::Right => out.push_str("\\tqr"),
            TabAlign::Decimal => out.push_str("\\tqdec"),
            TabAlign::Left => {}
        }
        let _ = write!(out, "\\tx{}", t.pos);
    }
    let flags = [
        (p.keep_with_next, "\\keepn"),
        (p.keep_together, "\\keep"),
        (p.page_break_before, "\\pagebb"),
        (p.suppress_line_numbers, "\\noline"),
    ];
    for (on, word) in flags {
        if on {
            out.push_str(word);
        }
    }
    let b = &p.borders;
    border(out, "t", &b.top, tables);
    border(out, "b", &b.bottom, tables);
    border(out, "l", &b.left, tables);
    border(out, "r", &b.right, tables);
    border(out, "btw", &b.between, tables);
    if b.shadow && !b.is_none() {
        out.push_str("\\brdrsh");
    }
    if let Some(color) = p.shading_color {
        let _ = write!(out, "\\cbpat{}", tables.index(color));
    }
    if p.shading > 0 {
        // RTF shading is in hundredths of a percent.
        let _ = write!(out, "\\shading{}", p.shading as u32 * 10);
    }
    out.push(' ');
}

/// A picture as a `\pict` group: the file's bytes in hex, with its goal
/// size, scale and cropping.
fn pict(out: &mut String, pic: &quill_core::Picture) {
    let kind = match pic.format {
        quill_core::ImageFormat::Png => "\\pngblip",
        quill_core::ImageFormat::Jpeg => "\\jpegblip",
    };
    let (sx, sy) = pic.scale();
    let (gw, gh) = pic.natural;
    let crop = |v: i32, whole: i32| {
        (v as i64 * whole as i64 / quill_core::picture::CROP_WHOLE as i64) as i32
    };
    let c = &pic.crop;
    let _ = writeln!(
        out,
        "{{\\pict{kind}\\picw{}\\pich{}\\picwgoal{gw}\\pichgoal{gh}\\picscalex{}\\picscaley{}\\piccropl{}\\piccropt{}\\piccropr{}\\piccropb{}\\quillsizew{}\\quillsizeh{}\\quillcropl{}\\quillcropt{}\\quillcropr{}\\quillcropb{}",
        pic.pixels.0,
        pic.pixels.1,
        sx.round() as i32,
        sy.round() as i32,
        crop(c.left, gw),
        crop(c.top, gh),
        crop(c.right, gw),
        crop(c.bottom, gh),
        pic.size.0,
        pic.size.1,
        c.left,
        c.top,
        c.right,
        c.bottom,
    );
    for (i, b) in pic.data.iter().enumerate() {
        let _ = write!(out, "{b:02x}");
        if i % 64 == 63 {
            out.push('\n');
        }
    }
    out.push('}');
}

/// A paragraph of the text, or (`in_note`) of a footnote, where a
/// reference is the note's own mark.
/// `nested` is the row of a row mark in a table inside a cell, for its
/// definition: a nested row is defined where it ends.
fn paragraph(
    out: &mut String,
    doc: &Document,
    p: &Paragraph,
    tables: &Tables,
    in_note: bool,
    nested: Option<&RowProps>,
) {
    if p.mark == Mark::RowEnd {
        // The end of a row is a paragraph mark with its own format, or
        // it would take the last cell's.
        out.push_str("\\pard\\plain");
        pap(out, &p.pap, tables);
        out.push_str("\\plain");
        run_chp(out, doc, p, &p.mark_chp, tables);
        match nested {
            // Programs that cannot nest tables read the group after it.
            Some(row) => {
                out.push_str("{\\*\\nesttableprops");
                row_definition(out, row, tables);
                out.push_str("\\nestrow}{\\nonesttables\\par}\n");
            }
            None => out.push_str("\\row\n"),
        }
        return;
    }
    out.push_str("\\pard\\plain");
    pap(out, &p.pap, tables);
    // A hyperlink is a HYPERLINK field whose result holds its runs.
    let mut link = 0;
    let main = crate::docx::write::main_index(doc, p);
    for (s, e, c) in p.run_bounds() {
        tables.comment_marks(out, main.map(|i| (i, s)), true);
        if c.link != link {
            if link != 0 {
                out.push_str("}}");
            }
            link = c.link;
            if let Some(l) = quill_core::link::get(link) {
                let mut instr = format!(
                    " HYPERLINK \"{}\" ",
                    crate::docx::write::field_quote(&l.url)
                );
                if l.new_window {
                    instr.push_str("\\t \"_blank\" ");
                }
                out.push_str("{\\field{\\*\\fldinst {");
                text(out, &instr);
                out.push_str("}}{\\fldrslt ");
            } else {
                link = 0;
            }
        }
        out.push('{');
        run_chp(out, doc, p, c, tables);
        let run = &p.text[s..e];
        match doc
            .picture(c.object)
            .filter(|_| run.contains(chars::OBJECT))
        {
            Some(pic) => {
                for _ in run.chars() {
                    pict(out, pic);
                }
            }
            None if run.contains([chars::FOOTNOTE, chars::ANNOTATION]) => {
                let mut from = 0;
                for (k, ch) in run.char_indices() {
                    if ch != chars::FOOTNOTE && ch != chars::ANNOTATION {
                        continue;
                    }
                    text(out, &run[from..k]);
                    if ch == chars::FOOTNOTE {
                        note_ref(out, doc, c.object, tables, in_note);
                    } else {
                        annotation_ref(out, doc, c.object, tables, in_note);
                    }
                    from = k + ch.len_utf8();
                }
                text(out, &run[from..]);
            }
            None => text(out, run),
        }
        out.push('}');
        tables.comment_marks(out, main.map(|i| (i, e)), false);
    }
    if link != 0 {
        out.push_str("}}");
    }
    // The mark's own format goes just before it.
    out.push_str("\\plain");
    run_chp(out, doc, p, &p.mark_chp, tables);
    out.push_str(match p.mark {
        Mark::CellEnd if p.pap.nest > 0 => "\\nestcell\n",
        Mark::CellEnd => "\\cell\n",
        Mark::SectionBreak => "\\sect\n",
        _ => "\\par\n",
    });
}

/// A footnote reference: `\\chftn` (or the note's own mark), and in the
/// text the note itself as a `\\footnote` group.
fn note_ref(out: &mut String, doc: &Document, id: u32, tables: &Tables, in_note: bool) {
    match doc.note_marks.get(&id) {
        Some(m) => text(out, m),
        None => out.push_str("\\chftn "),
    }
    if in_note {
        return;
    }
    out.push_str("{\\footnote");
    if let Some(r) = doc.note_ranges().get(&id) {
        story(out, doc, &doc.notes.paras[r.clone()], tables, true);
    }
    out.push('}');
}

/// An annotation mark: who wrote it, `\chatn`, and in the text the
/// annotation itself as an `\annotation` group.
fn annotation_ref(out: &mut String, doc: &Document, id: u32, tables: &Tables, in_note: bool) {
    if in_note {
        out.push_str("\\chatn ");
        return;
    }
    let info = doc.annotation_info.get(&id).cloned().unwrap_or_default();
    out.push_str("{\\*\\atnid ");
    text(out, &info.initials);
    out.push_str("}{\\*\\atnauthor ");
    text(out, &info.author);
    out.push_str("}{\\*\\quillatndate ");
    text(out, &info.date);
    out.push('}');
    // Replies and resolved state are Quill's own: Word's RTF has neither.
    if let Some(parent) = info.parent {
        let _ = write!(out, "{{\\*\\quillatnparent {parent}}}");
    }
    if info.done {
        out.push_str("{\\*\\quillatndone}");
    }
    let _ = write!(out, "\\chatn {{\\*\\annotation{{\\*\\atnref {id}}}");
    if let Some(r) = doc.annotation_ranges().get(&id) {
        story(out, doc, &doc.annotations.paras[r.clone()], tables, true);
    }
    out.push('}');
}

/// A section's settings, after `\sectd` resets them. `landscape` marks a
/// page wider than it is tall, which Word reads its orientation from.
fn section(out: &mut String, s: &quill_core::SectionProps, landscape: bool) {
    use quill_core::{PageNumberFormat as F, SectionStart as S};
    out.push_str(match s.start {
        S::NoBreak => "\\sectd\\sbknone",
        S::NewColumn => "\\sectd\\sbkcol",
        S::NewPage => "\\sectd\\sbkpage",
        S::EvenPage => "\\sectd\\sbkeven",
        S::OddPage => "\\sectd\\sbkodd",
    });
    let _ = write!(
        out,
        "\\cols{}\\colsx{}\\headery{}\\footery{}",
        s.columns.max(1),
        s.column_spacing,
        s.header_from_top,
        s.footer_from_bottom
    );
    out.push_str(match s.page_number_format {
        F::Arabic => "\\pgndec",
        F::UpperRoman => "\\pgnucrm",
        F::LowerRoman => "\\pgnlcrm",
        F::UpperLetter => "\\pgnucltr",
        F::LowerLetter => "\\pgnlcltr",
    });
    if s.restart_page_numbers {
        out.push_str("\\pgnrestart");
    }
    if s.margin_page_numbers {
        let _ = write!(
            out,
            "\\pgnx{}\\pgny{}",
            s.page_number_from_right, s.page_number_from_top
        );
    }
    if landscape {
        out.push_str("\\lndscpsxn");
    }
    if s.different_first_page {
        out.push_str("\\titlepg");
    }
    if s.line_numbers != quill_core::LineNumbering::Off {
        let _ = write!(out, "\\linemod{}", s.line_count_by.max(1));
        if let Some(t) = s.line_from_text {
            let _ = write!(out, "\\linex{t}");
        }
        out.push_str(match s.line_numbers {
            quill_core::LineNumbering::ByPage => "\\lineppage",
            quill_core::LineNumbering::BySection => "\\linerestart",
            _ => "\\linecont",
        });
    }
    out.push('\n');
}

fn border_style(style: BorderStyle) -> u8 {
    match style {
        BorderStyle::None => 0,
        BorderStyle::Dotted => 1,
        BorderStyle::Hairline => 2,
        BorderStyle::Single => 3,
        BorderStyle::Thick => 4,
        BorderStyle::Double => 5,
    }
}

/// Standard RTF row geometry, plus an ignorable exact Quill record for the
/// cell border/shading details that older RTF readers vary in interpreting.
fn row_definition(out: &mut String, row: &RowProps, tables: &Tables) {
    let (height_kind, height, trrh) = match row.height {
        RowHeight::Auto => (0, 0, 0),
        RowHeight::AtLeast(h) => (1, h, h),
        RowHeight::Exactly(h) => (2, h, -h),
    };
    let alignment = match row.align {
        Align::Left | Align::Justify => 0,
        Align::Center => 1,
        Align::Right => 2,
    };
    let _ = write!(
        out,
        "\\trowd\\trgaph{}\\trleft{}\\trrh{}{}",
        row.gap / 2,
        row.indent - row.gap / 2,
        trrh,
        match row.align {
            Align::Center => "\\trqc",
            Align::Right => "\\trqr",
            _ => "\\trql",
        }
    );
    let mut edge = row.indent - row.gap / 2;
    for cell in &row.cells {
        edge += cell.width;
        if let Some(color) = cell.shading_color {
            let _ = write!(out, "\\clcbpat{}", tables.index(color));
        }
        if cell.shading > 0 {
            let _ = write!(out, "\\clshdng{}", cell.shading as u32 * 10);
        }
        match cell.vmerge {
            VMerge::None => {}
            VMerge::Restart => out.push_str("\\clvmgf"),
            VMerge::Continue => out.push_str("\\clvmrg"),
        }
        match cell.valign {
            quill_core::props::VAlign::Top => {}
            quill_core::props::VAlign::Center => out.push_str("\\clvertalc"),
            quill_core::props::VAlign::Bottom => out.push_str("\\clvertalb"),
        }
        for (word, line) in [
            ("t", &cell.borders.top),
            ("b", &cell.borders.bottom),
            ("l", &cell.borders.left),
            ("r", &cell.borders.right),
        ] {
            if !line.is_none() {
                let _ = write!(out, "\\clbrdr{word}");
                border_words(out, line, tables);
            }
        }
        let _ = write!(out, "\\cellx{edge}");
    }
    let _ = write!(
        out,
        "{{\\*\\quillrow {},{},{height_kind},{height},{alignment}",
        row.gap, row.indent
    );
    for cell in &row.cells {
        let _ = write!(
            out,
            "|{},{},{}",
            cell.width,
            cell.shading,
            u8::from(cell.borders.shadow)
        );
        for line in [
            &cell.borders.top,
            &cell.borders.bottom,
            &cell.borders.left,
            &cell.borders.right,
            &cell.borders.between,
        ] {
            let _ = write!(out, ",{},{}", border_style(line.style), line.space);
        }
        // The 14th number, vertical alignment, is left out when Top
        // unless a 15th, the vertical merge, follows it.
        let valign = match cell.valign {
            quill_core::props::VAlign::Top => 0,
            quill_core::props::VAlign::Center => 1,
            quill_core::props::VAlign::Bottom => 2,
        };
        let vmerge = match cell.vmerge {
            VMerge::None => 0,
            VMerge::Restart => 1,
            VMerge::Continue => 2,
        };
        if valign != 0 || vmerge != 0 {
            let _ = write!(out, ",{valign}");
        }
        if vmerge != 0 {
            let _ = write!(out, ",{vmerge}");
        }
        // Then the lines' colors, when any is not black.
        let b = &cell.borders;
        let lines = [&b.top, &b.bottom, &b.left, &b.right, &b.between];
        if lines.iter().any(|l| l.color != Rgb::BLACK) {
            let colors: Vec<String> = lines.iter().map(|l| l.color.0.to_string()).collect();
            let _ = write!(out, ";{}", colors.join(","));
        }
        if let Some(color) = cell.shading_color {
            let _ = write!(out, ":{}", color.0);
        }
    }
    out.push_str("}\n");
}

/// Before the first paragraph of a table row, the row's definition. A
/// table inside a cell has its rows defined where they end instead.
fn row_start(out: &mut String, doc: &Document, paras: &[Paragraph], i: usize, tables: &Tables) {
    let starts_row = i == 0
        || paras[i - 1].pap.level() == 0
        || (paras[i - 1].mark == Mark::RowEnd && paras[i - 1].pap.level() == 1);
    if paras[i].pap.level() >= 1
        && starts_row
        && let Some(row) = quill_core::table::row_at_level(paras, i, 1)
    {
        let props = quill_core::table::row_props_merged(paras, &row, doc.props.text_width());
        row_definition(out, &props, tables);
    }
}

/// The row a nested row's end mark closes, for its definition.
fn nested_row(doc: &Document, paras: &[Paragraph], i: usize) -> Option<RowProps> {
    let p = &paras[i];
    if p.mark != Mark::RowEnd || p.pap.nest == 0 {
        return None;
    }
    let row = quill_core::table::row_at_level(paras, i, p.pap.level())?;
    Some(quill_core::table::row_props_merged(
        paras,
        &row,
        doc.props.text_width(),
    ))
}

/// The paragraphs of a header, footer, note or annotation, with their
/// tables' rows defined as in the main text.
fn story(out: &mut String, doc: &Document, paras: &[Paragraph], tables: &Tables, in_note: bool) {
    for (i, p) in paras.iter().enumerate() {
        row_start(out, doc, paras, i, tables);
        paragraph(
            out,
            doc,
            p,
            tables,
            in_note,
            nested_row(doc, paras, i).as_ref(),
        );
    }
}

/// A section's six header/footer stories. RTF uses the left-page
/// destinations for Quill's even-page variants.
fn headers(out: &mut String, doc: &Document, section: usize, tables: &Tables) {
    if section > 0 && !doc.has_own_headers(section) {
        return;
    }
    for (id, destination) in [
        (StoryId::Header, "header"),
        (StoryId::Footer, "footer"),
        (StoryId::FirstHeader, "headerf"),
        (StoryId::FirstFooter, "footerf"),
        (StoryId::EvenHeader, "headerl"),
        (StoryId::EvenFooter, "footerl"),
    ] {
        out.push_str("{\\");
        out.push_str(destination);
        out.push(' ');
        story(
            out,
            doc,
            &doc.header_story(section, id).paras,
            tables,
            false,
        );
        out.push_str("}\n");
    }
}

pub fn write(doc: &Document) -> Vec<u8> {
    let mut colors: Vec<Rgb> = Vec::new();
    let mut authors = vec!["Unknown".to_string()];
    let mut note = |c: &CharProps| {
        let mut add = |c: Rgb| {
            if c != Rgb::BLACK && !colors.contains(&c) {
                colors.push(c);
            }
        };
        add(c.color);
        // A tracked change's authors, and the color a format change
        // came from.
        if let Some(r) = quill_core::revision::get(c.rev) {
            let by = [&r.inserted, &r.deleted]
                .into_iter()
                .flatten()
                .chain(r.formatted.as_ref().map(|f| &f.0));
            for by in by {
                if !authors.contains(&by.author) {
                    authors.push(by.author.clone());
                }
            }
            if let Some((_, old)) = &r.formatted {
                add(old.color);
            }
        }
    };
    // Every story's colors: the main text, notes, annotations, and each
    // section's headers and footers.
    let section_headers = doc
        .main
        .paras
        .iter()
        .filter_map(|p| p.section.as_ref()?.headers.as_ref())
        .flat_map(|h| h.iter());
    let stories = [&doc.main, &doc.notes, &doc.annotations]
        .into_iter()
        .chain(&doc.headers)
        .chain(section_headers);
    let mut line_colors: Vec<Rgb> = Vec::new();
    for p in stories.flat_map(|s| &s.paras) {
        line_colors.extend(p.pap.shading_color);
        line_colors.extend(
            p.row
                .iter()
                .flat_map(|r| &r.cells)
                .filter_map(|c| c.shading_color),
        );
        note(&p.mark_chp);
        p.runs.iter().for_each(|r| note(&r.chp));
        let cells = p.row.iter().flat_map(|row| &row.cells);
        for b in std::iter::once(&p.pap.borders).chain(cells.map(|c| &c.borders)) {
            line_colors.extend([b.top, b.bottom, b.left, b.right, b.between].map(|l| l.color));
        }
    }
    doc.styles.styles.iter().for_each(|s| note(&s.chp));
    doc.styles
        .characters
        .iter()
        .for_each(|s| note(&s.format.props));
    line_colors.extend(doc.styles.styles.iter().filter_map(|s| s.pap.shading_color));
    for c in line_colors {
        if !colors.contains(&c) {
            colors.push(c);
        }
    }
    let tables = Tables {
        colors,
        authors,
        comments: crate::docx::write::comment_edges(doc),
    };

    let mut out = String::from("{\\rtf1\\ansi\\ansicpg1252\\uc1\\deff0\n{\\fonttbl");
    for (i, f) in doc.fonts.iter().enumerate() {
        let _ = write!(out, "{{\\f{i}\\fnil ");
        // A semicolon ends the name, so one in it is written escaped.
        for (k, part) in f.split(';').enumerate() {
            if k > 0 {
                out.push_str("\\u59?");
            }
            text(&mut out, part);
        }
        out.push_str(";}");
    }
    out.push_str("}\n{\\colortbl;");
    for c in &tables.colors {
        let _ = write!(out, "\\red{}\\green{}\\blue{};", c.r(), c.g(), c.b());
    }
    out.push_str("}\n{\\stylesheet");
    for s in &doc.styles.styles {
        out.push('{');
        pap(&mut out, &s.pap, &tables);
        if let Some(b) = s.based_on {
            let _ = write!(out, "\\sbasedon{b}");
        }
        let _ = write!(out, "\\snext{}", s.next);
        chp(&mut out, &s.chp, &tables);
        // A semicolon ends the name, so one in it is written escaped.
        for (k, part) in s.name.split(';').enumerate() {
            if k > 0 {
                out.push_str("\\u59?");
            }
            text(&mut out, part);
        }
        out.push_str(";}\n");
    }
    for (i, style) in doc.styles.characters.iter().enumerate() {
        let _ = write!(
            out,
            "{{\\*\\cs{}\\additive",
            doc.styles.styles.len() + i + 1
        );
        character_values(&mut out, &style.format.props, style.format.mask, &tables);
        out.push(' ');
        for (k, part) in style.name.split(';').enumerate() {
            if k > 0 {
                out.push_str("\\u59?");
            }
            text(&mut out, part);
        }
        out.push_str(";}\n");
    }
    out.push('}');
    if tables.authors.len() > 1 {
        out.push_str("\n{\\*\\revtbl ");
        for a in &tables.authors {
            out.push('{');
            // A semicolon ends the name, so one in it is written escaped.
            for (k, part) in a.split(';').enumerate() {
                if k > 0 {
                    out.push_str("\\u59?");
                }
                text(&mut out, part);
            }
            out.push_str(";}");
        }
        out.push('}');
    }
    out.push_str("\n{\\info");
    let s = &doc.summary;
    for (tag, value) in [
        ("title", &s.title),
        ("subject", &s.subject),
        ("author", &s.author),
        ("keywords", &s.keywords),
    ] {
        if !value.is_empty() {
            let _ = write!(out, "{{\\{tag} ");
            text(&mut out, value);
            out.push('}');
        }
    }
    if !s.version.is_empty() {
        out.push_str("{\\*\\quillversion ");
        text(&mut out, &s.version);
        out.push('}');
    }
    let p = &doc.props;
    // Word takes the orientation from these flags, not from the sizes, so
    // without them it calls a wide page portrait.
    let landscape = p.page_width > p.page_height;
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
    let _ = write!(
        out,
        "}}\n\\paperw{}\\paperh{}{}\\margl{}\\margr{}\\margt{top}\\margb{bottom}\\gutter{}\\deftab{}{}{}\n",
        p.page_width,
        p.page_height,
        if landscape { "\\landscape" } else { "" },
        p.margin_left,
        p.margin_right,
        p.gutter,
        p.default_tab,
        if p.widow_control {
            "\\widowctrl"
        } else {
            "\\widowctrl0"
        },
        if p.mirror_margins { "\\margmirror" } else { "" },
    );
    if let Some(view) = p.view {
        let kind = match view {
            SavedView::Normal => 4,
            SavedView::Outline => 2,
            SavedView::PageLayout => 1,
        };
        let _ = writeln!(out, "\\viewkind{kind}");
    }
    if p.print_hidden {
        out.push_str("\\printhidden");
    }
    if p.track_changes {
        out.push_str("\\revisions");
    }
    if p.even_odd_headers {
        out.push_str("\\facingp");
    }
    // Footnote settings.
    out.push_str(match p.footnote_position {
        quill_core::document::FootnotePosition::BottomOfPage => "\\ftnbj",
        quill_core::document::FootnotePosition::BeneathText => "\\ftntj",
        quill_core::document::FootnotePosition::EndOfSection => "\\endnotes",
        quill_core::document::FootnotePosition::EndOfDocument => "\\enddoc",
    });
    let _ = write!(out, "\\ftnstart{}", p.footnote_start.max(1));
    if p.footnote_restart_each_page {
        out.push_str("\\ftnrstpg");
    }
    out.push('\n');
    // Each section's settings come before its text.
    let mut section_index = 0;
    section(&mut out, doc.section_props_at(0), landscape);
    headers(&mut out, doc, section_index, &tables);
    for (i, para) in doc.main.paras.iter().enumerate() {
        row_start(&mut out, doc, &doc.main.paras, i, &tables);
        paragraph(
            &mut out,
            doc,
            para,
            &tables,
            false,
            nested_row(doc, &doc.main.paras, i).as_ref(),
        );
        if para.mark == Mark::SectionBreak && i + 1 < doc.main.paras.len() {
            section_index += 1;
            section(&mut out, doc.section_props_at(i + 1), landscape);
            headers(&mut out, doc, section_index, &tables);
        }
    }
    out.push('}');
    out.into_bytes()
}
