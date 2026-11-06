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
    let mut d = Dialog::new("Document Setup", 501.0, 185.0);
    d.group("Margins", 10.0, 7.0, 359.0, 87.0);
    d.text("Left:", 15.0, 25.0);
    d.field("left", &units::length(p.margin_left), 70.0, 22.0, 69.0);
    d.text("Right:", 15.0, 50.0);
    d.field("right", &units::length(p.margin_right), 70.0, 47.0, 69.0);
    d.text("Gutter:", 15.0, 73.0);
    d.field("gutter", &units::length(p.gutter), 70.0, 70.0, 69.0);
    let rules = ["At Least", "Exactly"];
    d.text("Top:", 151.0, 25.0);
    d.field("top", &units::length(p.margin_top), 208.0, 22.0, 69.0);
    d.popup("top_rule", &rules, p.top_exact as usize, 283.0, 22.0, 79.0);
    d.text("Bottom:", 151.0, 50.0);
    d.field("bottom", &units::length(p.margin_bottom), 208.0, 47.0, 69.0);
    d.popup(
        "bottom_rule",
        &rules,
        p.bottom_exact as usize,
        283.0,
        47.0,
        79.0,
    );
    d.check("mirror", "Mirror Even/Odd", 153.0, 72.0, p.mirror_margins);

    d.group("Footnotes", 10.0, 97.0, 218.0, 78.0);
    d.text("Position:", 15.0, 115.0);
    let names: Vec<&str> = FOOTNOTE_POSITIONS.iter().map(|f| f.0).collect();
    let at = FOOTNOTE_POSITIONS
        .iter()
        .position(|f| f.1 == p.footnote_position)
        .unwrap_or(0);
    d.popup("fn_pos", &names, at, 78.0, 112.0, 144.0);
    d.radio(
        "fn_restart",
        "Restart Each Page",
        17.0,
        133.0,
        1,
        p.footnote_restart_each_page,
    );
    d.radio(
        "fn_from",
        "Number From:",
        17.0,
        153.0,
        1,
        !p.footnote_restart_each_page,
    );
    d.field(
        "fn_start",
        &p.footnote_start.to_string(),
        127.0,
        150.0,
        58.0,
    );

    d.check("widow", "Widow Control", 237.0, 105.0, p.widow_control);
    d.check("hidden", "Print Hidden Text", 237.0, 121.0, p.print_hidden);
    d.check(
        "evenodd",
        "Even/Odd Headers",
        237.0,
        138.0,
        p.even_odd_headers,
    );
    d.text("Default Tab Stops:", 237.0, 156.0);
    d.field("deftab", &units::length(p.default_tab), 358.0, 154.0, 67.0);

    d.button("ok", "OK", 382.0, 16.0, 101.0, true);
    d.button("cancel", "Cancel", 382.0, 42.0, 101.0, false);
    d.button("usedefault", "Use As Default", 382.0, 66.0, 101.0, false);
    d.button("fileseries", "File Series…", 382.0, 90.0, 101.0, false);
    d.disable("fileseries");
    d.focus("left");
    d
}

/// The settings the Document dialog shows, or None if a field is not a
/// valid measurement.
fn read_document(d: &Dialog, base: &DocumentProps) -> Option<DocumentProps> {
    let mut p = base.clone();
    let len = |id| units::parse_length(&d.field_text(id));
    p.margin_left = len("left")?;
    p.margin_right = len("right")?;
    p.margin_top = len("top")?.max(0);
    p.margin_bottom = len("bottom")?.max(0);
    p.gutter = len("gutter")?.max(0);
    p.top_exact = d.selected("top_rule") == 1;
    p.bottom_exact = d.selected("bottom_rule") == 1;
    p.mirror_margins = d.is_on("mirror");
    p.footnote_position = FOOTNOTE_POSITIONS[d.selected("fn_pos").min(3)].1;
    p.footnote_restart_each_page = d.is_on("fn_restart");
    p.footnote_start = d.field_text("fn_start").trim().parse().ok()?;
    p.widow_control = d.is_on("widow");
    p.print_hidden = d.is_on("hidden");
    p.even_odd_headers = d.is_on("evenodd");
    p.default_tab = len("deftab")?;
    // Text needs some room between the margins.
    if p.default_tab <= 0 || p.text_width() < 720 {
        return None;
    }
    Some(p)
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

/// Paper sizes in twips, portrait.
pub(crate) const PAPERS: [(&str, i32, i32); 6] = [
    ("US Letter", 12240, 15840),
    ("US Legal", 12240, 20160),
    ("A4 Letter", 11906, 16838),
    ("A5 Letter", 8391, 11906),
    ("B5 Letter", 9979, 14175),
    ("Tabloid", 15840, 24480),
];

/// The paper choices: the standard sizes, then the document's own if it is
/// none of those.
fn papers(view: &DocView) -> Vec<(String, i32, i32)> {
    let mut v: Vec<(String, i32, i32)> = PAPERS
        .iter()
        .map(|(n, w, h)| (n.to_string(), *w, *h))
        .collect();
    let p = &view.doc.props;
    let (w, h) = (
        p.page_width.min(p.page_height),
        p.page_width.max(p.page_height),
    );
    if !v
        .iter()
        .any(|x| (x.1 - w).abs() < 20 && (x.2 - h).abs() < 20)
    {
        v.push((format!("{} x {}", units::length(w), units::length(h)), w, h));
    }
    v
}

/// File > Page Setup: paper size and orientation. (The original showed the
/// printer driver's dialog here, with its own Document… button added.)
pub fn page_setup(view: &DocView) -> Dialog {
    let p = &view.doc.props;
    let list = papers(view);
    let (w, h) = (
        p.page_width.min(p.page_height),
        p.page_width.max(p.page_height),
    );
    let at = list
        .iter()
        .position(|x| (x.1 - w).abs() < 20 && (x.2 - h).abs() < 20)
        .unwrap_or(0);
    let mut d = Dialog::new("Page Setup", 422.0, 112.0);
    d.text("Paper:", 12.0, 13.0);
    d.popup_owned(
        "paper",
        list.into_iter().map(|x| x.0).collect(),
        at,
        100.0,
        10.0,
        170.0,
    );
    d.text("Orientation:", 12.0, 45.0);
    let wide = p.page_width > p.page_height;
    d.radio("tall", "Tall", 100.0, 45.0, 1, !wide);
    d.radio("wide", "Wide", 170.0, 45.0, 1, wide);
    d.button("ok", "OK", 290.0, 10.0, 70.0, true);
    d.button("cancel", "Cancel", 290.0, 40.0, 70.0, false);
    d.button("document", "Document Setup…", 290.0, 80.0, 120.0, false);
    d
}

/// OK in Page Setup.
pub fn apply_page_setup(d: &Dialog, view: &mut DocView) -> bool {
    let list = papers(view);
    let Some(&(_, w, h)) = list.get(d.selected("paper")) else {
        return true;
    };
    let (w, h) = if d.is_on("wide") { (h, w) } else { (w, h) };
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
pub fn print(view: &DocView) -> Dialog {
    let mut d = Dialog::new("Print", 400.0, 132.0);
    d.text("Copies:", 12.0, 12.0);
    d.field("copies", "1", 80.0, 10.0, 40.0);
    d.text("Pages:", 12.0, 44.0);
    d.radio("all", "All", 80.0, 44.0, 1, true);
    d.radio("range", "From:", 130.0, 44.0, 1, false);
    d.field("from", "", 196.0, 42.0, 34.0);
    d.text("To:", 238.0, 44.0);
    d.field("to", "", 264.0, 42.0, 34.0);
    d.check(
        "hidden",
        "Print Hidden Text",
        12.0,
        76.0,
        view.doc.props.print_hidden,
    );
    d.button("ok", "Print", 316.0, 10.0, 72.0, true);
    d.button("cancel", "Cancel", 316.0, 40.0, 72.0, false);
    d.button("savepdf", "Save as PDF…", 270.0, 102.0, 118.0, false);
    d.focus("copies");
    d
}

/// The job the Print dialog describes, or None if a field is not a number.
pub fn print_job(d: &Dialog, view: &mut DocView, pdf: bool) -> Option<PrintJob> {
    let copies: u32 = d.field_text("copies").trim().parse().ok()?;
    let pages = if d.is_on("range") {
        let from: usize = d.field_text("from").trim().parse().unwrap_or(1);
        let to: usize = d.field_text("to").trim().parse().unwrap_or(usize::MAX);
        Some((from.max(1), to.max(from.max(1))))
    } else {
        None
    };
    let hidden = d.is_on("hidden");
    view.edit_document_props("Print Hidden Text", |p| p.print_hidden = hidden);
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
    let mut d = Dialog::new("Section", 492.0, 181.0);
    d.text("Start:", 11.0, 8.0);
    let names: Vec<&str> = STARTS.iter().map(|x| x.0).collect();
    let at = STARTS.iter().position(|x| x.1 == s.start).unwrap_or(0);
    d.popup("start", &names, at, 52.0, 6.0, 124.0);
    d.check(
        "endnotes",
        "Include Endnotes",
        192.0,
        8.0,
        s.include_endnotes,
    );
    // Only meaningful when footnotes go at the end of each section.
    if view.doc.props.footnote_position != FootnotePosition::EndOfSection {
        d.disable("endnotes");
    }

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

    d.button("ok", "OK", 367.0, 6.0, 110.0, true);
    d.button("cancel", "Cancel", 367.0, 40.0, 110.0, false);
    d.button("apply", "Apply", 367.0, 65.0, 110.0, false);
    d.button("usedefault", "Use As Default", 367.0, 118.0, 110.0, false);
    d.button("linenumbers", "Line Numbers…", 367.0, 142.0, 110.0, false);
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
    let len = |id| units::parse_length(&d.field_text(id));
    s.start = STARTS[d.selected("start").min(4)].1;
    s.include_endnotes = d.is_on("endnotes");
    s.columns = d.field_text("columns").trim().parse().ok()?;
    if !(1..=100).contains(&s.columns) {
        return None;
    }
    s.column_spacing = len("spacing")?.max(0);
    s.header_from_top = len("hdr_top")?.max(0);
    s.footer_from_bottom = len("ftr_bottom")?.max(0);
    s.different_first_page = d.is_on("firstpage");
    s.page_number_format = FORMATS[d.selected("format").min(4)].1;
    s.restart_page_numbers = d.is_on("restart");
    s.margin_page_numbers = d.is_on("margin_numbers");
    if s.margin_page_numbers {
        s.page_number_from_top = len("pn_top")?.max(0);
        s.page_number_from_right = len("pn_right")?.max(0);
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
            page_number_from_right,
            include_endnotes
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
    fn document_dialog_round_trips_the_settings() {
        let mut v = view();
        v.doc.props.margin_left = 2160;
        v.doc.props.bottom_exact = true;
        let d = document(&v);
        assert_eq!(d.field_text("left"), "1.5 in");
        assert_eq!(read_document(&d, &v.doc.props), Some(v.doc.props.clone()));
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
    fn print_dialog_reads_the_job() {
        let mut v = view();
        let mut d = print(&v);
        assert_eq!(
            print_job(&d, &mut v, false),
            Some(PrintJob {
                copies: 1,
                pages: None,
                pdf: false
            })
        );
        d.set_field("copies", "3");
        d.set_field("from", "2");
        print_changed(&mut d, "from");
        let job = print_job(&d, &mut v, true).unwrap();
        assert_eq!(job.copies, 3);
        assert_eq!(job.pages, Some((2, usize::MAX)));
        assert!(job.pdf);
        d.set_field("copies", "lots");
        assert_eq!(print_job(&d, &mut v, false), None);
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
