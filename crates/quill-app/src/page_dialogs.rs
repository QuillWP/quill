//! Format > Document and Format > Section. Layouts measured from the original.

use crate::dialog::{Dialog, units};
use crate::docview::DocView;
use quill_core::document::FootnotePosition;
use quill_core::{DocumentProps, PageNumberFormat, SectionProps, SectionStart};

const FOOTNOTE_POSITIONS: [(&str, FootnotePosition); 4] = [
    ("Bottom of Page", FootnotePosition::BottomOfPage),
    ("Beneath Text", FootnotePosition::BeneathText),
    ("End of Section", FootnotePosition::EndOfSection),
    ("End of Document", FootnotePosition::EndOfDocument),
];

pub fn document(view: &DocView) -> Dialog {
    let p = &view.doc.props;
    // Everything below the paper row sits 30 points lower than it did.
    let dy = 30.0;
    let mut d = Dialog::new("Document Setup", 501.0, 291.0 + dy);
    let at_paper = paper_row(&paper_choices(false), p.page_width, p.page_height);
    let names: Vec<String> = paper_choices(offers_manage())
        .into_iter()
        .map(|x| x.0)
        .collect();
    d.text("Paper:", 15.0, 14.0);
    d.popup_owned("paper", names, at_paper, 70.0, 10.0, 170.0);
    custom_size_fields(&mut d, 250.0, 10.0, p);
    d.group("Margins", 10.0, 7.0 + dy, 359.0, 87.0);
    d.text_as("left_label", "", 15.0, 25.0 + dy);
    d.field("left", &units::length(p.margin_left), 70.0, 22.0 + dy, 69.0);
    d.text_as("right_label", "", 15.0, 50.0 + dy);
    d.field(
        "right",
        &units::length(p.margin_right),
        70.0,
        47.0 + dy,
        69.0,
    );
    // Only shown with facing pages, where the gutter is part of the layout.
    d.text_as("gutter_label", "Gutter:", 15.0, 73.0 + dy);
    d.field("gutter", &units::length(p.gutter), 70.0, 70.0 + dy, 69.0);
    let rules = ["At Least", "Exactly"];
    d.text("Top:", 151.0, 25.0 + dy);
    d.field("top", &units::length(p.margin_top), 208.0, 22.0 + dy, 69.0);
    d.popup(
        "top_rule",
        &rules,
        p.top_exact as usize,
        283.0,
        22.0 + dy,
        79.0,
    );
    d.text("Bottom:", 151.0, 50.0 + dy);
    d.field(
        "bottom",
        &units::length(p.margin_bottom),
        208.0,
        47.0 + dy,
        69.0,
    );
    d.popup(
        "bottom_rule",
        &rules,
        p.bottom_exact as usize,
        283.0,
        47.0 + dy,
        79.0,
    );
    d.check("mirror", "Facing Pages", 153.0, 72.0 + dy, p.mirror_margins);
    // Where the margins are kept while Facing Pages is off, so that turning
    // it on again brings them back. Empty until it is turned off here.
    for id in ["mem_left", "mem_right", "mem_gutter"] {
        d.text_as(id, "", 0.0, 0.0);
        d.set_hidden(id, true);
    }
    d.tab_in_order = true;
    mirror_changed(&mut d);

    d.group("Footnotes", 10.0, 97.0 + dy, 218.0, 78.0);
    d.text("Position:", 15.0, 115.0 + dy);
    let names: Vec<&str> = FOOTNOTE_POSITIONS.iter().map(|f| f.0).collect();
    let at = FOOTNOTE_POSITIONS
        .iter()
        .position(|f| f.1 == p.footnote_position)
        .unwrap_or(0);
    d.popup("fn_pos", &names, at, 78.0, 112.0 + dy, 144.0);
    d.radio(
        "fn_restart",
        "Restart Each Page",
        17.0,
        133.0 + dy,
        1,
        p.footnote_restart_each_page,
    );
    d.radio(
        "fn_from",
        "Number From:",
        17.0,
        153.0 + dy,
        1,
        !p.footnote_restart_each_page,
    );
    d.field(
        "fn_start",
        &p.footnote_start.to_string(),
        127.0,
        150.0 + dy,
        58.0,
    );

    d.check(
        "evenodd",
        "Even/Odd Headers",
        237.0,
        105.0 + dy,
        p.even_odd_headers,
    );
    d.text("Default Tab Stops:", 237.0, 126.0 + dy);
    d.field(
        "deftab",
        &units::length(p.default_tab),
        358.0,
        124.0 + dy,
        67.0,
    );

    // How this document draws its superscripts and subscripts.
    let sc = p.scripts;
    d.group("Superscript and Subscript", 10.0, 182.0 + dy, 359.0, 64.0);
    d.text("Superscript: up", 15.0, 202.0 + dy);
    d.field("raise", &sc.raise.to_string(), 125.0, 199.0 + dy, 40.0);
    d.text("%, at", 169.0, 202.0 + dy);
    d.field(
        "supsize",
        &sc.super_size.to_string(),
        210.0,
        199.0 + dy,
        40.0,
    );
    d.text("% of the size", 254.0, 202.0 + dy);
    d.text("Subscript: down", 15.0, 226.0 + dy);
    d.field("drop", &sc.drop.to_string(), 125.0, 223.0 + dy, 40.0);
    d.text("%, at", 169.0, 226.0 + dy);
    d.field("subsize", &sc.sub_size.to_string(), 210.0, 223.0 + dy, 40.0);
    d.text("% of the size", 254.0, 226.0 + dy);

    d.button("usedefault", "Use As Default", 382.0, 16.0, 101.0, false);
    d.button("fileseries", "File Series…", 382.0, 90.0, 101.0, false);
    d.disable("fileseries");
    d.action_buttons(&[("cancel", "Cancel", 82.0, false), ("ok", "OK", 82.0, true)]);
    d.focus("left");
    d
}

/// The settings the Document dialog shows, or None if a field is not a
/// valid measurement.
fn read_document(d: &Dialog, base: &DocumentProps) -> Option<DocumentProps> {
    let mut p = base.clone();
    let len = |id, old| units::parse_length_from(&d.field_text(id), old);
    // With Facing Pages unticked here, the margin fields show the equal
    // margins, which the shown text only rounds; the exact ones are worked
    // out again from the facing margins that were remembered.
    let (old_left, old_right) = match remembered(d, base) {
        Some((l, r, g)) if !d.is_on("mirror") => equal_margins(l, r, g),
        _ => (base.margin_left, base.margin_right),
    };
    p.margin_left = len("left", old_left)?;
    p.margin_right = len("right", old_right)?;
    p.margin_top = len("top", base.margin_top)?.max(0);
    p.margin_bottom = len("bottom", base.margin_bottom)?.max(0);
    if d.is_on("mirror") {
        p.gutter = len("gutter", base.gutter)?.max(0);
        p.mirror_margins = true;
    } else {
        // Unticking Facing Pages already showed the equal margins, with the
        // gutter folded in (see `mirror_toggled`), so they are taken as shown.
        p.gutter = 0;
        p.mirror_margins = false;
    }
    // Only a changed paper size is applied, and a standard size keeps the
    // orientation the document has. Margins that no longer fit the paper are
    // brought in, as File > Page Setup does.
    let (w, h) = read_paper(d, base, None)?;
    if (w - base.page_width).abs() >= 20 || (h - base.page_height).abs() >= 20 {
        (p.page_width, p.page_height) = (w, h);
        p.keep_in_bounds();
    }
    p.top_exact = d.selected("top_rule") == 1;
    p.bottom_exact = d.selected("bottom_rule") == 1;
    p.footnote_position = FOOTNOTE_POSITIONS[d.selected("fn_pos").min(3)].1;
    p.footnote_restart_each_page = d.is_on("fn_restart");
    p.footnote_start = d
        .field_text("fn_start")
        .trim()
        .parse()
        .ok()
        .filter(|&n| n > 0)?;
    p.even_odd_headers = d.is_on("evenodd");
    p.default_tab = len("deftab", base.default_tab)?;
    // A percentage left as shown keeps its value; one out of range is
    // brought within it.
    let pct = |id: &str| -> Option<u16> {
        d.field_text(id)
            .trim()
            .trim_end_matches('%')
            .trim()
            .parse()
            .ok()
    };
    p.scripts = quill_core::ScriptSizes {
        raise: pct("raise")?,
        super_size: pct("supsize")?,
        drop: pct("drop")?,
        sub_size: pct("subsize")?,
    }
    .clamped();
    // Text needs some room between the margins.
    if p.default_tab <= 0 || p.text_width() < 720 {
        return None;
    }
    Some(p)
}

/// Mirror Even/Odd changes which edges the two fields name.
pub fn mirror_changed(d: &mut Dialog) {
    let (left, right) = if d.is_on("mirror") {
        ("Inside:", "Outside:")
    } else {
        ("Left:", "Right:")
    };
    d.set_label("left_label", left);
    d.set_label("right_label", right);
    let on = d.is_on("mirror");
    d.set_hidden("gutter_label", !on);
    d.set_hidden("gutter", !on);
}

/// Left and right margins that are equal and add up to the same total.
/// An odd twip goes to the left, as in `DocumentProps::set_facing_pages`.
fn equal_margins(left: i32, right: i32, gutter: i32) -> (i32, i32) {
    let total = left.saturating_add(right).saturating_add(gutter);
    (total - total / 2, total / 2)
}

/// The facing margins kept when Facing Pages was unticked, if it was.
fn remembered(d: &Dialog, base: &DocumentProps) -> Option<(i32, i32, i32)> {
    if d.item_label("mem_left").is_empty() {
        return None;
    }
    let len = |id, old| units::parse_length_from(d.item_label(id), old);
    Some((
        len("mem_left", base.margin_left)?,
        len("mem_right", base.margin_right)?,
        len("mem_gutter", base.gutter)?,
    ))
}

/// Facing Pages was ticked or unticked in the Document dialog. Unticking
/// shows the equal margins the document would get (the same total, so the
/// text keeps its width) and remembers the facing margins; ticking again puts
/// them back.
pub fn mirror_toggled(d: &mut Dialog, base: &DocumentProps) {
    if d.is_on("mirror") {
        if !d.item_label("mem_left").is_empty() {
            for (field, mem) in [
                ("left", "mem_left"),
                ("right", "mem_right"),
                ("gutter", "mem_gutter"),
            ] {
                let text = d.item_label(mem).to_string();
                d.set_field(field, &text);
                d.set_label(mem, "");
            }
        }
    } else if d.item_label("mem_left").is_empty() {
        let len = |id, old| units::parse_length_from(&d.field_text(id), old);
        if let (Some(l), Some(r), Some(g)) = (
            len("left", base.margin_left),
            len("right", base.margin_right),
            len("gutter", base.gutter),
        ) {
            for (mem, field) in [
                ("mem_left", "left"),
                ("mem_right", "right"),
                ("mem_gutter", "gutter"),
            ] {
                let text = d.field_text(field);
                d.set_label(mem, &text);
            }
            let (left, right) = equal_margins(l, r, g);
            d.set_field("left", &units::length(left));
            d.set_field("right", &units::length(right));
            d.set_field("gutter", &units::length(0));
        }
    }
    mirror_changed(d);
}

/// OK in the Document dialog. False (nothing changed) if a field is bad.
pub fn apply_document(d: &Dialog, view: &mut DocView) -> bool {
    let Some(p) = read_document(d, &view.doc.props) else {
        return false;
    };
    view.set_document_props("Document", p);
    true
}

/// The Document dialog's settings, for Use As Default.
pub fn document_defaults(d: &Dialog, view: &DocView) -> Option<DocumentProps> {
    read_document(d, &view.doc.props)
}

/// Built-in paper sizes in twips, portrait. Used when the computer offers
/// none of its own.
pub(crate) const PAPERS: [(&str, i32, i32); 6] = [
    ("US Letter", 12240, 15840),
    ("US Legal", 12240, 20160),
    ("A4 Letter", 11906, 16838),
    ("A5 Letter", 8391, 11906),
    ("B5 Letter", 9979, 14175),
    ("Tabloid", 15840, 24480),
];

/// The standard paper sizes: the computer's, or the built-in ones.
pub(crate) fn standard_papers() -> Vec<(String, i32, i32)> {
    let v = crate::system_paper::system_papers();
    if !v.is_empty() {
        return v;
    }
    PAPERS
        .iter()
        .map(|(n, w, h)| (n.to_string(), *w, *h))
        .collect()
}

/// The paper choices: the standard sizes, then the custom ones defined in
/// the system, then "Custom" (no size of its own; the Width and Height
/// fields hold it). `manage` adds a last row, "Manage Custom Sizes…", which
/// is not a size.
pub(crate) fn paper_choices(manage: bool) -> Vec<(String, i32, i32)> {
    let mut v = standard_papers();
    v.extend(crate::system_paper::custom_papers());
    v.push(("Custom".to_string(), 0, 0));
    if manage {
        v.push((MANAGE.to_string(), 0, 0));
    }
    v
}

/// The Document dialog's pop-up row that opens the system's custom size
/// manager (the Mac's Page Setup panel), if this system has one.
pub(crate) const MANAGE: &str = "Manage Custom Sizes\u{2026}";

/// Whether the Document dialog offers `MANAGE`.
pub(crate) fn offers_manage() -> bool {
    cfg!(all(target_os = "macos", not(test)))
}

/// The pop-up row of Custom.
fn custom_row() -> usize {
    paper_choices(false).len() - 1
}

fn same_paper(x: &(String, i32, i32), w: i32, h: i32) -> bool {
    (x.1 - w).abs() < 20 && (x.2 - h).abs() < 20
}

/// The row of `list` (made without `manage`) that matches a page size, in
/// either orientation, or the Custom row (the last) if none does.
pub(crate) fn paper_row(list: &[(String, i32, i32)], width: i32, height: i32) -> usize {
    let (w, h) = (width.min(height), width.max(height));
    list[..list.len() - 1]
        .iter()
        .position(|x| same_paper(x, w, h))
        .unwrap_or(list.len() - 1)
}

/// True if the Document dialog's paper pop-up is on the manage row.
pub(crate) fn manage_picked(d: &Dialog) -> bool {
    offers_manage() && d.selected("paper") > custom_row()
}

/// Width and height fields next to the paper pop-up. They show the size of
/// the paper picked, and typing in them picks Custom (see `paper_changed`).
pub(crate) fn custom_size_fields(d: &mut Dialog, x: f32, y: f32, p: &DocumentProps) {
    d.field("paper_w", &units::length(p.page_width), x, y, 48.0);
    d.text_as("paper_x", "x", x + 52.0, y + 3.0);
    d.field("paper_h", &units::length(p.page_height), x + 64.0, y, 48.0);
}

/// The paper pop-up or a size field (`id`) changed. A new paper fills the
/// fields with its size, turned to `wide` if the page is wide. Typing a size
/// picks the listed paper of that size, or Custom.
pub(crate) fn paper_changed(d: &mut Dialog, id: &str, wide: bool, unit: crate::settings::Unit) {
    let list = paper_choices(false);
    let custom = list.len() - 1;
    let oriented = |x: &(String, i32, i32)| if wide { (x.2, x.1) } else { (x.1, x.2) };
    if id == "paper" {
        if let Some(x) = list[..custom].get(d.selected("paper")) {
            let (w, h) = oriented(x);
            d.set_field("paper_w", &units::length_in(unit, w));
            d.set_field("paper_h", &units::length_in(unit, h));
        }
        return;
    }
    let row = match (
        units::parse(&d.field_text("paper_w"), unit.abbrev()),
        units::parse(&d.field_text("paper_h"), unit.abbrev()),
    ) {
        (Some(w), Some(h)) => list[..custom]
            .iter()
            .position(|x| {
                let (pw, ph) = oriented(x);
                (pw - w).abs() < 20 && (ph - h).abs() < 20
            })
            .unwrap_or(custom),
        _ => custom,
    };
    d.set_selected("paper", row);
}

/// The default paper picked in Preferences, in the orientation `base` has.
pub(crate) fn read_default_paper(d: &Dialog, base: &DocumentProps) -> Option<(i32, i32)> {
    read_paper(d, base, Some(base.page_width > base.page_height))
}

/// The page size the paper row asks for, as (width, height). `wide` is the
/// orientation for a standard size; None keeps the page's own. A Custom size
/// is read as typed (turned to `wide` if given). None if a field is bad or
/// out of range.
fn read_paper(d: &Dialog, base: &DocumentProps, wide: Option<bool>) -> Option<(i32, i32)> {
    use quill_core::limits::{MAX_LENGTH, MIN_PAGE};
    let list = paper_choices(false);
    let at = d.selected("paper");
    let wide_now = base.page_width > base.page_height;
    // The manage row is not a size: the page stays as it is.
    if at > custom_row() {
        return Some((base.page_width, base.page_height));
    }
    if at == custom_row() {
        let w = units::parse_length_from(&d.field_text("paper_w"), base.page_width)?;
        let h = units::parse_length_from(&d.field_text("paper_h"), base.page_height)?;
        if !(MIN_PAGE..=MAX_LENGTH).contains(&w) || !(MIN_PAGE..=MAX_LENGTH).contains(&h) {
            return None;
        }
        return Some(match wide {
            Some(true) => (w.max(h), w.min(h)),
            Some(false) => (w.min(h), w.max(h)),
            None => (w, h),
        });
    }
    let &(_, w, h) = list.get(at)?;
    Some(if wide.unwrap_or(wide_now) {
        (h, w)
    } else {
        (w, h)
    })
}

/// File > Page Setup: paper size and orientation. (The original showed the
/// printer driver's dialog here, with its own Document… button added.)
pub fn page_setup(view: &DocView) -> Dialog {
    let p = &view.doc.props;
    let list = paper_choices(false);
    let at = paper_row(&list, p.page_width, p.page_height);
    let mut d = Dialog::new("Page Setup", 422.0, 146.0);
    d.text("Paper:", 12.0, 13.0);
    d.popup_owned(
        "paper",
        list.into_iter().map(|x| x.0).collect(),
        at,
        100.0,
        10.0,
        170.0,
    );
    custom_size_fields(&mut d, 278.0, 10.0, p);
    d.text("Orientation:", 12.0, 45.0);
    let wide = p.page_width > p.page_height;
    d.radio("tall", "Tall", 100.0, 45.0, 1, !wide);
    d.radio("wide", "Wide", 170.0, 45.0, 1, wide);
    d.button("document", "Document Setup…", 12.0, 80.0, 120.0, false);
    d.action_buttons(&[("cancel", "Cancel", 70.0, false), ("ok", "OK", 70.0, true)]);
    d
}

/// OK in Page Setup. False (nothing changed) if a custom size is bad.
pub fn apply_page_setup(d: &Dialog, view: &mut DocView) -> bool {
    let Some((w, h)) = read_paper(d, &view.doc.props, Some(d.is_on("wide"))) else {
        return false;
    };
    view.edit_document_props("Page Setup", |p| {
        p.page_width = w;
        p.page_height = h;
        // Margins set for a larger paper shrink to fit this one.
        p.keep_in_bounds();
    });
    true
}

/// What File > Print was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrintJob {
    pub copies: u32,
    /// First and last page, counting from 1; None for all of them.
    pub pages: Option<(usize, usize)>,
    /// Save a PDF file instead of printing.
    pub pdf: bool,
}

/// File > Print (⌘P). The original added its options to the printer driver's
/// dialog; this is Quill's own, with a Save as PDF… button.
pub fn print() -> Dialog {
    let mut d = Dialog::new("Print", 400.0, 156.0);
    d.text("Copies:", 12.0, 12.0);
    d.field("copies", "1", 80.0, 10.0, 40.0);
    d.text("Pages:", 12.0, 44.0);
    d.radio("all", "All", 80.0, 44.0, 1, true);
    d.radio("range", "From:", 130.0, 44.0, 1, false);
    d.field("from", "", 196.0, 42.0, 34.0);
    d.text("To:", 238.0, 44.0);
    d.field("to", "", 264.0, 42.0, 34.0);
    d.action_buttons(&[
        ("savepdf", "Save as PDF…", 118.0, false),
        ("cancel", "Cancel", 72.0, false),
        ("ok", "Print", 72.0, true),
    ]);
    d.focus("copies");
    d
}

/// The job the Print dialog describes, or None if a field is not a number.
pub fn print_job(d: &Dialog, pdf: bool) -> Option<PrintJob> {
    let copies: u32 = d.field_text("copies").trim().parse().ok()?;
    let pages = if d.is_on("range") {
        let from: usize = d.field_text("from").trim().parse().unwrap_or(1);
        let to: usize = d.field_text("to").trim().parse().unwrap_or(usize::MAX);
        Some((from.max(1), to.max(from.max(1))))
    } else {
        None
    };
    Some(PrintJob {
        copies: copies.clamp(1, 999),
        pages,
        pdf,
    })
}

/// Typing in From or To picks the range button, as on the Mac.
pub fn print_changed(d: &mut Dialog, id: &str) {
    if id == "from" || id == "to" {
        d.set_on("all", false);
        d.set_on("range", true);
    }
}

const STARTS: [(&str, SectionStart); 5] = [
    ("No Break", SectionStart::NoBreak),
    ("New Column", SectionStart::NewColumn),
    ("New Page", SectionStart::NewPage),
    ("Even Page", SectionStart::EvenPage),
    ("Odd Page", SectionStart::OddPage),
];

const FORMATS: [(&str, PageNumberFormat); 5] = [
    ("1 2 3", PageNumberFormat::Arabic),
    ("I II III", PageNumberFormat::UpperRoman),
    ("i ii iii", PageNumberFormat::LowerRoman),
    ("A B C", PageNumberFormat::UpperLetter),
    ("a b c", PageNumberFormat::LowerLetter),
];

pub fn section(view: &DocView) -> Dialog {
    let s = view.current_section();
    let mut d = Dialog::new("Section", 492.0, 217.0);
    d.text("Start:", 11.0, 8.0);
    let names: Vec<&str> = STARTS.iter().map(|x| x.0).collect();
    let at = STARTS.iter().position(|x| x.1 == s.start).unwrap_or(0);
    d.popup("start", &names, at, 52.0, 6.0, 124.0);

    d.group("Columns", 10.0, 30.0, 169.0, 62.0);
    d.text("Number:", 15.0, 43.0);
    d.field("columns", &s.columns.to_string(), 111.0, 40.0, 61.0);
    d.text("Spacing:", 15.0, 68.0);
    d.field(
        "spacing",
        &units::length(s.column_spacing),
        111.0,
        65.0,
        61.0,
    );

    d.group("Header/Footer", 10.0, 97.0, 169.0, 78.0);
    d.text("From Top:", 15.0, 111.0);
    d.field(
        "hdr_top",
        &units::length(s.header_from_top),
        111.0,
        108.0,
        61.0,
    );
    d.text("From Bottom:", 15.0, 137.0);
    d.field(
        "ftr_bottom",
        &units::length(s.footer_from_bottom),
        111.0,
        134.0,
        61.0,
    );
    d.check(
        "firstpage",
        "Different First Page",
        15.0,
        157.0,
        s.different_first_page,
    );

    d.group("Page Numbers", 185.0, 30.0, 170.0, 131.0);
    d.text("Format:", 194.0, 47.0);
    let names: Vec<&str> = FORMATS.iter().map(|x| x.0).collect();
    let at = FORMATS
        .iter()
        .position(|x| x.1 == s.page_number_format)
        .unwrap_or(0);
    d.popup("format", &names, at, 265.0, 45.0, 80.0);
    d.check(
        "restart",
        "Restart at 1",
        192.0,
        70.0,
        s.restart_page_numbers,
    );
    d.check(
        "margin_numbers",
        "Margin Page Numbers",
        192.0,
        87.0,
        s.margin_page_numbers,
    );
    d.text_as("pn_top_label", "From Top:", 194.0, 111.0);
    d.field(
        "pn_top",
        &units::length(s.page_number_from_top),
        278.0,
        108.0,
        70.0,
    );
    d.text_as("pn_right_label", "From Right:", 194.0, 137.0);
    d.field(
        "pn_right",
        &units::length(s.page_number_from_right),
        278.0,
        134.0,
        70.0,
    );
    margin_numbers_changed(&mut d);

    d.button("usedefault", "Use As Default", 367.0, 40.0, 110.0, false);
    d.button("linenumbers", "Line Numbers…", 367.0, 142.0, 110.0, false);
    d.action_buttons(&[
        ("cancel", "Cancel", 90.0, false),
        ("apply", "Apply", 90.0, false),
        ("ok", "OK", 90.0, true),
    ]);
    d.focus("columns");
    d
}

/// From Top and From Right only apply to margin page numbers.
pub fn margin_numbers_changed(d: &mut Dialog) {
    let on = d.is_on("margin_numbers");
    for id in ["pn_top_label", "pn_top", "pn_right_label", "pn_right"] {
        d.enable(id, on);
    }
}

fn read_section(d: &Dialog, base: &SectionProps) -> Option<SectionProps> {
    let mut s = base.clone();
    let len = |id, old| units::parse_length_from(&d.field_text(id), old);
    s.start = STARTS[d.selected("start").min(4)].1;
    s.columns = d.field_text("columns").trim().parse().ok()?;
    if !(1..=quill_core::limits::MAX_COLUMNS).contains(&s.columns) {
        return None;
    }
    s.column_spacing = len("spacing", base.column_spacing)?.max(0);
    s.header_from_top = len("hdr_top", base.header_from_top)?.max(0);
    s.footer_from_bottom = len("ftr_bottom", base.footer_from_bottom)?.max(0);
    s.different_first_page = d.is_on("firstpage");
    s.page_number_format = FORMATS[d.selected("format").min(4)].1;
    s.restart_page_numbers = d.is_on("restart");
    s.margin_page_numbers = d.is_on("margin_numbers");
    if s.margin_page_numbers {
        s.page_number_from_top = len("pn_top", base.page_number_from_top)?.max(0);
        s.page_number_from_right = len("pn_right", base.page_number_from_right)?.max(0);
    }
    Some(s)
}

/// OK or Apply in the Section dialog.
pub fn apply_section(d: &Dialog, view: &mut DocView) -> bool {
    let base = view.current_section().clone();
    let Some(s) = read_section(d, &base) else {
        return false;
    };
    // Only what was changed goes to every selected section, so sections
    // that differ keep their other settings.
    view.format_sections(|p| {
        macro_rules! take {
            ($($f:ident),*) => {$(
                if s.$f != base.$f {
                    p.$f = s.$f.clone();
                }
            )*};
        }
        take!(
            start,
            columns,
            column_spacing,
            header_from_top,
            footer_from_bottom,
            different_first_page,
            page_number_format,
            restart_page_numbers,
            margin_page_numbers,
            page_number_from_top,
            page_number_from_right
        );
    });
    true
}

pub fn section_defaults(d: &Dialog, view: &DocView) -> Option<SectionProps> {
    read_section(d, view.current_section())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Cmd;
    use crate::docview::Clipboard;
    use quill_core::{Document, Position, Selection};

    fn view() -> DocView {
        DocView::new(Document::new(), "Test".into())
    }

    #[test]
    fn mirrored_margin_fields_change_labels_without_changing_values() {
        let mut view = DocView::new(quill_core::Document::new(), String::new());
        view.doc.props.margin_left = 2160;
        view.doc.props.margin_right = 1440;
        view.doc.props.gutter = 360;
        let mut dialog = document(&view);
        assert_eq!(dialog.item_label("left_label"), "Left:");
        assert_eq!(dialog.item_label("right_label"), "Right:");
        dialog.set_on("mirror", true);
        mirror_toggled(&mut dialog, &view.doc.props);
        assert_eq!(dialog.item_label("left_label"), "Inside:");
        assert_eq!(dialog.item_label("right_label"), "Outside:");
        let props = read_document(&dialog, &view.doc.props).unwrap();
        assert_eq!(props.page_margins(0), (2520, 1440));
        assert_eq!(props.page_margins(1), (1440, 2520));
        dialog.set_on("mirror", false);
        mirror_toggled(&mut dialog, &view.doc.props);
        assert_eq!(dialog.item_label("left_label"), "Left:");
        assert_eq!(dialog.item_label("right_label"), "Right:");
        // Clearing the box shows equal margins with the same total.
        let props = read_document(&dialog, &view.doc.props).unwrap();
        assert_eq!(props.page_margins(1), (1980, 1980));
    }

    #[test]
    fn facing_pages_off_in_the_dialog_keeps_the_text_width() {
        let mut view = DocView::new(quill_core::Document::new(), String::new());
        view.doc.props.mirror_margins = true;
        view.doc.props.margin_left = 2160;
        view.doc.props.margin_right = 1440;
        view.doc.props.gutter = 360;
        view.page_view = true;
        let width = view.doc.props.text_width();
        let mut dialog = document(&view);
        dialog.set_on("mirror", false);
        mirror_toggled(&mut dialog, &view.doc.props);
        assert!(apply_document(&dialog, &mut view));
        assert!(!view.doc.props.mirror_margins);
        assert!(!view.facing_pages());
        assert_eq!(view.doc.props.text_width(), width);
    }

    #[test]
    fn unticking_facing_pages_equalizes_and_ticking_restores() {
        let mut view = DocView::new(quill_core::Document::new(), String::new());
        view.doc.props.mirror_margins = true;
        view.doc.props.margin_left = 2160;
        view.doc.props.margin_right = 1440;
        view.doc.props.gutter = 360;
        let base = view.doc.props.clone();
        let mut d = document(&view);
        for _ in 0..2 {
            d.set_on("mirror", false);
            mirror_toggled(&mut d, &base);
            assert_eq!(d.field_text("left"), units::length(1980));
            assert_eq!(d.field_text("right"), units::length(1980));
            d.set_on("mirror", true);
            mirror_toggled(&mut d, &base);
            assert_eq!(d.field_text("left"), units::length(2160));
            assert_eq!(d.field_text("right"), units::length(1440));
            assert_eq!(d.field_text("gutter"), units::length(360));
        }
        // OK with the box left ticked changes nothing.
        assert_eq!(read_document(&d, &base), Some(base.clone()));
    }

    #[test]
    fn document_dialog_round_trips_the_settings() {
        let mut v = view();
        v.doc.props.margin_left = 2160;
        v.doc.props.bottom_exact = true;
        let d = document(&v);
        assert_eq!(d.field_text("left"), "1.5 in");
        assert_eq!(read_document(&d, &v.doc.props), Some(v.doc.props.clone()));
    }

    #[test]
    fn document_dialog_sets_the_document_s_superscript_sizes() {
        let mut v = view();
        let mut d = document(&v);
        assert_eq!(d.field_text("supsize"), "67");
        d.set_field("supsize", "100%");
        d.set_field("raise", "500");
        assert!(apply_document(&d, &mut v));
        let sc = v.doc.props.scripts;
        assert_eq!(
            (sc.raise, sc.super_size, sc.drop, sc.sub_size),
            (100, 100, 17, 67)
        );
        d.set_field("drop", "lots");
        assert!(!apply_document(&d, &mut v));
    }

    #[test]
    fn document_and_section_ok_without_changes_keep_metric_lengths() {
        // 2 cm and 1.25 cm show as "0.79 in" and "0.49 in", which would
        // read back as 1138 and 706 twips.
        let mut v = view();
        v.doc.props.margin_left = 1134;
        v.doc.props.default_tab = 709;
        let d = document(&v);
        assert_eq!(read_document(&d, &v.doc.props), Some(v.doc.props.clone()));
        // 0.75 cm shows as "0.3 in".
        v.doc.section.column_spacing = 425;
        let d = section(&v);
        assert_eq!(d.field_text("spacing"), "0.3 in");
        let s = v.current_section().clone();
        assert_eq!(read_section(&d, &s), Some(s.clone()));
    }

    #[test]
    fn document_dialog_refuses_bad_margins() {
        let mut v = view();
        let mut d = document(&v);
        d.set_field("left", "lots");
        assert!(!apply_document(&d, &mut v));
        d.set_field("left", "4 in");
        d.set_field("right", "4.25 in");
        assert!(!apply_document(&d, &mut v), "no room left for text");
        d.set_field("left", "2 in");
        d.set_field("right", "1");
        assert!(apply_document(&d, &mut v));
        assert_eq!(v.doc.props.margin_left, 2880);
        assert!(v.doc.is_modified());
    }

    #[test]
    fn dialogs_refuse_values_the_document_limits_would_change() {
        let v = view();
        let mut document = document(&v);
        document.set_field("fn_start", "0");
        assert!(read_document(&document, &v.doc.props).is_none());

        let mut section = section(&v);
        section.set_field(
            "columns",
            &(quill_core::limits::MAX_COLUMNS + 1).to_string(),
        );
        assert!(read_section(&section, &v.doc.section).is_none());
    }

    #[test]
    fn print_dialog_reads_the_job() {
        let mut d = print();
        assert_eq!(
            print_job(&d, false),
            Some(PrintJob {
                copies: 1,
                pages: None,
                pdf: false,
            })
        );
        d.set_field("copies", "3");
        d.set_field("from", "2");
        print_changed(&mut d, "from");
        let job = print_job(&d, true).unwrap();
        assert_eq!(job.copies, 3);
        assert_eq!(job.pages, Some((2, usize::MAX)));
        assert!(job.pdf);
        d.set_field("copies", "lots");
        assert_eq!(print_job(&d, false), None);
    }

    #[test]
    fn page_setup_sets_paper_and_orientation() {
        let mut v = view();
        let mut d = page_setup(&v);
        assert_eq!(d.selected_text("paper"), "US Letter");
        d.set_selected("paper", 2);
        d.set_on("tall", false);
        d.set_on("wide", true);
        assert!(apply_page_setup(&d, &mut v));
        assert_eq!(
            (v.doc.props.page_width, v.doc.props.page_height),
            (16838, 11906)
        );
        let d = page_setup(&v);
        assert_eq!(d.selected_text("paper"), "A4 Letter");
        assert!(d.is_on("wide"));
    }

    #[test]
    fn custom_paper_size_is_read_from_the_fields() {
        let mut v = view();
        let mut d = document(&v);
        // The fields follow the paper picked, and typing in them picks Custom.
        d.set_selected("paper", 2);
        paper_changed(&mut d, "paper", false, crate::settings::get().unit);
        assert_eq!(d.field_text("paper_w"), units::length(11906));
        d.set_field("paper_w", "5 in");
        paper_changed(&mut d, "paper_w", false, crate::settings::get().unit);
        assert_eq!(d.selected_text("paper"), "Custom");
        d.set_field("paper_w", "5 in");
        d.set_field("paper_h", "8 in");
        assert!(apply_document(&d, &mut v));
        assert_eq!(
            (v.doc.props.page_width, v.doc.props.page_height),
            (7200, 11520)
        );
        // The dialog now opens on Custom, and OK without edits changes nothing.
        let d = document(&v);
        assert_eq!(d.selected_text("paper"), "Custom");
        assert!(apply_document(&d, &mut v));
        assert_eq!(v.doc.props.page_width, 7200);
        // Too small a page is refused.
        let mut d = document(&v);
        d.set_field("paper_w", "0.2 in");
        assert!(!apply_document(&d, &mut v));
    }

    #[test]
    fn section_dialog_reads_its_fields() {
        let mut v = view();
        let mut d = section(&v);
        assert!(!d.is_enabled("pn_top"));
        d.set_field("columns", "2");
        d.set_selected("start", 2);
        d.set_selected("format", 2);
        assert!(apply_section(&d, &mut v));
        assert_eq!(v.doc.section.columns, 2);
        assert_eq!(v.doc.section.start, SectionStart::NewPage);
        assert_eq!(
            v.doc.section.page_number_format,
            PageNumberFormat::LowerRoman
        );
        d.set_field("columns", "0");
        assert!(!apply_section(&d, &mut v));
        v.doc.undo();
        assert_eq!(v.doc.section.columns, 1);
    }

    #[test]
    fn section_dialog_changes_the_section_at_the_caret() {
        let mut v = view();
        v.type_text("one");
        v.exec(Cmd::SectionBreak, false, &mut Clipboard::new());
        v.type_text("two");
        v.set_selection(Selection::caret(Position::new(0, 0)));
        let mut d = section(&v);
        d.set_field("columns", "3");
        assert!(apply_section(&d, &mut v));
        assert_eq!(v.doc.section_props_at(0).columns, 3);
        assert_eq!(v.doc.section.columns, 1);
        v.doc.undo();
        assert_eq!(v.doc.section_props_at(0).columns, 1);
    }
}
