//! Insert > Table, Format > Table Cells and Format > Table Layout. Layouts
//! measured from the original.

use crate::dialog::{Dialog, units};
use crate::docview::DocView;
use quill_core::para::Mark;
use quill_core::props::{Align, BorderLine, BorderStyle, Rgb, RowHeight, RowProps, Twips, VAlign};
use quill_core::table::{self, Convert, Row};
use quill_core::{Position, Selection, StoryId};

const MAIN: StoryId = StoryId::Main;

/// The table cells a selection covers: the rows it touches and the range
/// of cell numbers (taken from where it starts and ends).
#[derive(Clone, Debug)]
pub struct TableSel {
    pub rows: Vec<Row>,
    pub from: usize,
    pub to: usize,
}

pub fn table_selection(view: &DocView) -> Option<TableSel> {
    let (s, e) = view.sel.range();
    let paras = &view.doc.main.paras;
    let first = table::row_at(paras, s.para)?;
    let rows: Vec<Row> = table::table_rows(paras, s.para)
        .into_iter()
        .filter(|r| r.end >= s.para && r.first <= e.para.max(s.para))
        .collect();
    let a = first.cell_of(s.para).unwrap_or(first.cells.len() - 1);
    let last = rows.last().unwrap_or(&first);
    let b = last
        .cell_of(e.para)
        .unwrap_or(last.cells.len().saturating_sub(1));
    let (from, to) = if rows.len() > 1 {
        (a.min(b), a.max(b))
    } else {
        (a, b.max(a))
    };
    Some(TableSel { rows, from, to })
}

// ----- Insert Table -----

/// The room a new table has where the insertion point is: the text
/// column, or the text of the cell it is in.
fn available_width(view: &DocView) -> Twips {
    let p = view.sel.focus.para;
    let text_width = view.doc.column_width_at(p);
    let paras = &view.doc.main.paras;
    let Some(row) = table::row_at(paras, p) else {
        return text_width;
    };
    let k = row.cell_of(p).unwrap_or(row.cells.len() - 1);
    let props = table::row_props(paras, &row, text_width);
    // Rows of tables inside cells are fitted to their cell by layout, so
    // the room is bounded by the outer cells too.
    let own = (props.cells[k].width - props.gap).max(360);
    match table::row_at_level(paras, p, 1).filter(|outer| outer.first != row.first) {
        Some(outer) => {
            let outer_props = table::row_props(paras, &outer, text_width);
            let j = outer.cell_of(p).unwrap_or(outer.cells.len() - 1);
            own.min((outer_props.cells[j].width - outer_props.gap).max(360))
        }
        None => own,
    }
}

/// What Insert Table will do: make a new table, or turn the selected
/// paragraphs into one.
#[derive(Clone, Debug)]
pub struct InsertState {
    /// The selected paragraphs, when there is text to convert.
    pub convert: Option<(usize, usize)>,
}

/// Down the first column, then the second.
const CONVERTS: [(&str, &str); 4] = [
    ("c_paras", "Paragraphs"),
    ("c_tabs", "Tab Delimited"),
    ("c_commas", "Comma Delimited"),
    ("c_side", "Side by Side Only"),
];

pub fn insert_table(view: &DocView) -> (Dialog, InsertState) {
    let (s, e) = view.sel.range();
    let paras = &view.doc.main.paras;
    let convert = (!view.sel.is_empty() && !paras[s.para].pap.in_table).then(|| {
        // A selection ending at a paragraph's very start leaves it out.
        let last = if e.offset == 0 && e.para > s.para {
            e.para - 1
        } else {
            e.para
        };
        (s.para, last)
    });
    let (mut cols, mut rows) = (2, 2);
    let mut tabs = false;
    if let Some((a, b)) = convert {
        tabs = paras[a..=b].iter().any(|p| p.text.contains('\t'));
        if tabs {
            cols = paras[a..=b]
                .iter()
                .map(|p| p.text.matches('\t').count() + 1)
                .max()
                .unwrap_or(1);
            rows = b - a + 1;
        } else {
            cols = 1;
            rows = b - a + 1;
        }
    }
    let width = available_width(view) / cols as Twips;
    let mut d = Dialog::new("Insert Table", 318.0, 150.0);
    d.text("Number of Columns:", 13.0, 16.0);
    d.field("cols", &cols.to_string(), 151.0, 14.0, 67.0);
    d.text("Number of Rows:", 13.0, 40.0);
    d.field("rows", &rows.to_string(), 151.0, 38.0, 67.0);
    d.text("Column Width:", 13.0, 65.0);
    d.field("width", &units::length(width), 151.0, 63.0, 67.0);
    d.button("ok", "OK", 231.0, 17.0, 69.0, true);
    d.button("cancel", "Cancel", 231.0, 42.0, 69.0, false);
    d.button("format", "Format…", 231.0, 65.0, 69.0, false);
    d.group("Convert From", 12.0, 88.0, 291.0, 53.0);
    // Two columns of choices, as in the original.
    for (k, (id, label)) in CONVERTS.into_iter().enumerate() {
        let x = if k < 2 { 15.0 } else { 150.0 };
        let y = if k % 2 == 0 { 103.0 } else { 119.0 };
        let on = match k {
            0 => !tabs,
            1 => tabs,
            _ => false,
        };
        d.radio(id, label, x, y, 1, on);
        if convert.is_none() {
            d.disable(id);
        }
    }
    d.focus("cols");
    (d, InsertState { convert })
}

/// Keep the column width in step with the number of columns.
pub fn insert_changed(view: &DocView, d: &mut Dialog, id: &str) {
    if id == "cols"
        && let Ok(n) = d.field_text("cols").trim().parse::<usize>()
        && n > 0
    {
        let w = available_width(view) / n as Twips;
        d.set_field("width", &units::length(w));
    }
}

/// The columns, rows and column width Insert Table was asked for, or why
/// they will not do.
fn insert_values(st: &InsertState, d: &Dialog) -> Result<(usize, usize, Twips), &'static str> {
    // Converting tab or comma separated text takes the number of columns
    // from the text, so what the fields say about it does not matter.
    let from_text = st.convert.is_some() && !matches!(d.radio_on(1), "c_paras");
    let cols: usize = match d.field_text("cols").trim().parse() {
        Ok(n) if (1..=31).contains(&n) => n,
        _ if from_text => 1,
        _ => return Err("Number of Columns must be a number from 1 to 31."),
    };
    let rows: usize = match d.field_text("rows").trim().parse() {
        Ok(n) if (1..=999).contains(&n) => n,
        _ if from_text => 1,
        _ => return Err("Number of Rows must be a number from 1 to 999."),
    };
    match units::parse_length(&d.field_text("width")) {
        Some(w) if w >= 360 => Ok((cols, rows, w)),
        // The width the dialog filled in for a wide row of text is under
        // the minimum; the table then divides the text width instead.
        _ if from_text => Ok((cols, rows, 0)),
        _ => Err("Column Width must be at least 0.25 in."),
    }
}

/// How wide the table would be, and how much room there is for it (the
/// text column, or the cell it goes in), when it would not fit.
fn insert_too_wide(
    st: &InsertState,
    d: &Dialog,
    view: &DocView,
    cols: usize,
    width: Twips,
) -> Option<(Twips, Twips)> {
    // A converted table takes as many columns as its text has; one that
    // got no width of its own divides the room instead.
    let columns = match st.convert {
        Some((a, b)) if width > 0 => {
            let paras = &view.doc.main.paras[a..=b];
            let most = |sep: char| paras.iter().map(|p| p.text.matches(sep).count() + 1).max();
            match d.radio_on(1) {
                "c_tabs" => most('\t').unwrap_or(1),
                "c_commas" => most(',').unwrap_or(1),
                "c_side" => paras.len(),
                _ => cols,
            }
        }
        Some(_) => return None,
        None => cols,
    };
    let total = width.saturating_mul(columns as Twips);
    let room = available_width(view);
    // A table made to fill the room can come out a few twips over.
    (total > room + 20).then_some((total, room))
}

/// OK (or Format…): insert the table. False if a field is bad.
pub fn insert_apply(st: &InsertState, d: &Dialog, view: &mut DocView) -> bool {
    let (cols, rows, width) = match insert_values(st, d) {
        Ok(values) => values,
        Err(why) => {
            #[cfg(not(test))]
            crate::app::alert("Quill cannot insert this table.", why);
            let _ = why;
            return false;
        }
    };
    if let Some((total, room)) = insert_too_wide(st, d, view, cols, width) {
        let place = if view.current_pap().in_table {
            "the cell"
        } else if view.doc.section_props_at(view.sel.focus.para).columns > 1 {
            "the column"
        } else {
            "the page"
        };
        #[cfg(not(test))]
        if !crate::app::ask(
            &format!("The table is wider than {place}."),
            &format!(
                "Its columns add up to {}, but there is room for {}. Insert it anyway?",
                units::length(total),
                units::length(room)
            ),
            "Insert",
        ) {
            return false;
        }
        #[cfg(test)]
        let _ = (total, room, place);
    }
    view.doc.begin("Insert Table", view.sel);
    let before = view.doc.main.paras.len();
    let at = match st.convert {
        Some((a, b)) => {
            let how = match d.radio_on(1) {
                "c_tabs" => Convert::Tabs,
                "c_commas" => Convert::Commas,
                "c_side" => Convert::SideBySide,
                _ => Convert::Paragraphs(cols),
            };
            let text_width = view.doc.column_width_at(a);
            view.doc.convert_to_table(MAIN, a, b, how, |n| {
                if width > 0 {
                    width
                } else {
                    (text_width / n as Twips).max(144)
                }
            })
        }
        None => {
            let at = view.sel.focus;
            if view.sel.is_empty() {
                view.doc.insert_table(MAIN, at, rows, cols, width)
            } else {
                at
            }
        }
    };
    if view.doc.main.paras.len() != before {
        border_new_table(view, at);
    }
    view.set_selection(Selection::caret(at));
    let sel = view.sel;
    view.doc.commit(sel);
    true
}

/// The line a new table is drawn with, from settings: from then on it
/// belongs to the table, and Format > Border changes it.
fn new_table_line() -> BorderLine {
    crate::settings::with(|s| BorderLine {
        style: if s.table_border_dotted {
            BorderStyle::Dotted
        } else {
            BorderStyle::Hairline
        },
        space: 0,
        color: Rgb(s.table_border_color & 0xFF_FFFF),
    })
}

/// Give the table just made at `at` its borders, in the open undo step.
/// Rows beside it that were there already have their own, or none on
/// purpose, and are left alone.
pub fn border_new_table(view: &mut DocView, at: Position) {
    let line = new_table_line();
    let tw = view.doc.column_width_at(at.para);
    let rows = table::table_rows(&view.doc.main.paras, at.para);
    for row in rows.iter().filter(|r| r.first >= at.para) {
        let mut p = table::row_props(&view.doc.main.paras, row, tw);
        if p.cells.iter().any(|c| !c.borders.is_none()) {
            break;
        }
        for cell in &mut p.cells {
            cell.borders.top = line;
            cell.borders.bottom = line;
            cell.borders.left = line;
            cell.borders.right = line;
        }
        view.doc.set_row(MAIN, row.end, p);
    }
}

// ----- Convert to Table -----

/// What Convert to Table would do with the selected lines: which lines,
/// how they split into cells, and how many lines just above and below
/// look like more rows of the same table.
#[derive(Clone, Debug, PartialEq)]
pub struct ConvertPlan {
    pub first: usize,
    pub last: usize,
    pub how: Convert,
    pub above: usize,
    pub below: usize,
}

/// The selected tab or comma separated lines, if there are any. Tabs win
/// when a line has both, as in Insert Table.
pub fn convert_plan(view: &DocView) -> Option<ConvertPlan> {
    let (s, e) = view.sel.range();
    let paras = &view.doc.main.paras;
    if view.sel.is_empty() || paras[s.para].pap.in_table {
        return None;
    }
    // A selection ending at a paragraph's very start leaves it out.
    let last = if e.offset == 0 && e.para > s.para {
        e.para - 1
    } else {
        e.para
    };
    let selected = &paras[s.para..=last];
    if selected.iter().any(|p| p.pap.in_table) {
        return None;
    }
    let (sep, how) = if selected.iter().any(|p| p.text.contains('\t')) {
        ('\t', Convert::Tabs)
    } else if selected.iter().any(|p| p.text.contains(',')) {
        (',', Convert::Commas)
    } else {
        return None;
    };
    let fields = |p: &quill_core::para::Paragraph| p.text.matches(sep).count() + 1;
    // Neighbors are only offered when the selected lines all have the
    // same number of fields to match them against.
    let n = fields(&selected[0]);
    let uniform = selected.iter().all(|p| fields(p) == n);
    let like_table = |p: &quill_core::para::Paragraph| {
        uniform && !p.pap.in_table && p.mark == Mark::Paragraph && fields(p) == n && n > 1
    };
    let above = (0..s.para)
        .rev()
        .take_while(|&i| like_table(&paras[i]))
        .count();
    let below = (last + 1..paras.len())
        .take_while(|&i| like_table(&paras[i]))
        .count();
    Some(ConvertPlan {
        first: s.para,
        last,
        how,
        above,
        below,
    })
}

/// Make the plan's lines (and, if asked, the lines beside them that
/// matched) into a table. Each column is as wide as the text allows.
pub fn convert_lines(view: &mut DocView, plan: &ConvertPlan, with_neighbors: bool) {
    let (a, b) = if with_neighbors {
        (plan.first - plan.above, plan.last + plan.below)
    } else {
        (plan.first, plan.last)
    };
    let text_width = view.doc.column_width_at(a);
    view.doc.begin("Convert to Table", view.sel);
    let at = view
        .doc
        .convert_to_table(MAIN, a, b, plan.how, |n| (text_width / n as Twips).max(144));
    border_new_table(view, at);
    view.set_selection(Selection::caret(at));
    let sel = view.sel;
    view.doc.commit(sel);
}

/// Convert to Table in the right-click menu: when lines beside the
/// selection look like part of the same table, ask whether to include them.
pub fn convert_command(view: &mut DocView) {
    let Some(plan) = convert_plan(view) else {
        return;
    };
    let mut with_neighbors = false;
    if plan.above + plan.below > 0 {
        let lines = |n: usize| {
            if n == 1 {
                "1 line".to_string()
            } else {
                format!("{n} lines")
            }
        };
        let place = match (plan.above, plan.below) {
            (a, 0) => format!("{} above", lines(a)),
            (0, b) => format!("{} below", lines(b)),
            (a, b) => format!("{} above and {} below", lines(a), lines(b)),
        };
        #[cfg(not(test))]
        match crate::app::ask_include(
            "Text next to the selection looks like part of the same table.",
            &format!("Include {place} in the table?"),
            "Include",
            "Selection Only",
        ) {
            Some(include) => with_neighbors = include,
            None => return,
        }
        #[cfg(test)]
        let _ = (place, &mut with_neighbors);
    }
    convert_lines(view, &plan, with_neighbors);
}

// ----- Table Cells -----

const APPLY_TO: [&str; 4] = [
    "Selection",
    "Each Cell In Table",
    "Entire Columns Selected",
    "Entire Rows Selected",
];

const HALIGN: [&str; 4] = ["Left", "Center", "Right", "Justified"];
const VALIGN: [&str; 3] = ["Top", "Center", "Bottom"];

/// The first selected cell's alignment, as indexes into VALIGN and HALIGN.
fn cell_alignment(view: &DocView, ts: &TableSel) -> (usize, usize) {
    let paras = &view.doc.main.paras;
    let row = &ts.rows[0];
    let k = ts.from.min(row.cells.len() - 1);
    let props = table::row_props(paras, row, view.doc.column_width_at(row.first));
    let v = match props.cells[k].valign {
        VAlign::Top => 0,
        VAlign::Center => 1,
        VAlign::Bottom => 2,
    };
    let h = match paras[row.cells[k].0].pap.align {
        Align::Left => 0,
        Align::Center => 1,
        Align::Right => 2,
        Align::Justify => 3,
    };
    (v, h)
}

/// The selected rows' layouts, and the column width if the selected cells
/// share one.
fn selected_rows(view: &DocView, ts: &TableSel) -> (Vec<RowProps>, Option<Twips>) {
    let paras = &view.doc.main.paras;
    let tw = view.doc.column_width_at(ts.rows[0].first);
    let props: Vec<RowProps> = ts
        .rows
        .iter()
        .map(|r| table::row_props(paras, r, tw))
        .collect();
    let widths: Vec<Twips> = props
        .iter()
        .flat_map(|p| {
            p.cells[ts.from.min(p.cells.len() - 1)..=ts.to.min(p.cells.len() - 1)]
                .iter()
                .map(|c| c.width)
        })
        .collect();
    let width = widths
        .windows(2)
        .all(|w| w[0] == w[1])
        .then(|| widths.first().copied())
        .flatten();
    (props, width)
}

pub fn table_cells(view: &DocView) -> Option<Dialog> {
    let ts = table_selection(view)?;
    let (props, width) = selected_rows(view, &ts);
    let width = width.map(units::length).unwrap_or_default();
    let p = &props[0];
    let mut d = Dialog::new("Table Cells", 352.0, 262.0);
    d.text("Apply To:", 15.0, 9.0);
    d.popup("applyto", &APPLY_TO, 0, 82.0, 7.0, 170.0);
    d.text("Column Width:", 15.0, 34.0);
    d.field("width", &width, 186.0, 32.0, 65.0);
    d.group("Row", 11.0, 54.0, 249.0, 121.0);
    d.text("Height:", 17.0, 73.0);
    let (rule, value) = match p.height {
        RowHeight::Auto => (0, String::new()),
        RowHeight::AtLeast(t) => (1, units::points(t)),
        RowHeight::Exactly(t) => (2, units::points(t)),
    };
    d.popup(
        "height",
        &["Auto", "At Least", "Exactly"],
        rule,
        90.0,
        72.0,
        82.0,
    );
    d.field("height_by", &value, 185.0, 70.0, 66.0);
    d.text("Space Between Columns:", 17.0, 99.0);
    d.field("gap", &units::length(p.gap), 185.0, 97.0, 66.0);
    d.text("Indent:", 17.0, 126.0);
    d.field("indent", &units::length(p.indent), 185.0, 124.0, 66.0);
    d.text("Alignment:", 17.0, 152.0);
    let al = match p.align {
        Align::Center => 1,
        Align::Right => 2,
        _ => 0,
    };
    d.popup(
        "align",
        &["Left", "Center", "Right"],
        al,
        173.0,
        151.0,
        79.0,
    );
    let (v, h) = cell_alignment(view, &ts);
    d.group("Text In Cell", 11.0, 182.0, 249.0, 72.0);
    d.text("Horizontal:", 17.0, 201.0);
    d.popup("halign", &HALIGN, h, 120.0, 199.0, 132.0);
    d.text("Vertical:", 17.0, 227.0);
    d.popup("valign", &VALIGN, v, 120.0, 225.0, 132.0);
    d.button("ok", "OK", 271.0, 11.0, 69.0, true);
    d.button("cancel", "Cancel", 271.0, 40.0, 69.0, false);
    d.button("apply", "Apply", 271.0, 64.0, 69.0, false);
    d.button("border", "Border…", 271.0, 88.0, 69.0, false);
    d.focus("width");
    Some(d)
}

/// OK or Apply in Table Cells. False if a field is bad.
pub fn table_cells_apply(d: &Dialog, view: &mut DocView) -> bool {
    let Some(ts) = table_selection(view) else {
        return true;
    };
    // Fields left as they were shown keep their exact values.
    let (props, shown_width) = selected_rows(view, &ts);
    let len = |id, old: Option<Twips>| {
        let t = d.field_text(id);
        if t.trim().is_empty() {
            Ok(None)
        } else {
            match old {
                Some(old) => units::parse_length_from(&t, old),
                None => units::parse_length(&t),
            }
            .map(Some)
            .ok_or(())
        }
    };
    let (Ok(width), Ok(gap), Ok(indent)) = (
        len("width", shown_width),
        len("gap", Some(props[0].gap)),
        len("indent", Some(props[0].indent)),
    ) else {
        return false;
    };
    let by = units::parse(&d.field_text("height_by"), "pt")
        .unwrap_or(0)
        .abs();
    let height = match d.selected("height") {
        1 if by > 0 => RowHeight::AtLeast(by),
        2 if by > 0 => RowHeight::Exactly(by),
        _ => RowHeight::Auto,
    };
    let align = [Align::Left, Align::Center, Align::Right][d.selected("align").min(2)];
    // Text alignment changes only if it was changed here, so cells that
    // differ keep their own when other settings are applied.
    let (v0, h0) = cell_alignment(view, &ts);
    let (v1, h1) = (d.selected("valign").min(2), d.selected("halign").min(3));
    let valign = (v1 != v0).then(|| [VAlign::Top, VAlign::Center, VAlign::Bottom][v1]);
    let halign = (h1 != h0).then(|| [Align::Left, Align::Center, Align::Right, Align::Justify][h1]);
    let paras = &view.doc.main.paras;
    let all_rows = table::table_rows(paras, ts.rows[0].first);
    let mode = d.selected("applyto");
    let rows: Vec<Row> = if mode == 1 || mode == 2 {
        all_rows
    } else {
        ts.rows.clone()
    };
    let cols = |n: usize| -> std::ops::RangeInclusive<usize> {
        match mode {
            1 | 3 => 0..=n - 1,
            _ => ts.from.min(n - 1)..=ts.to.min(n - 1),
        }
    };
    let tw = view.doc.column_width_at(rows[0].first);
    view.doc.begin("Format Cells", view.sel);
    for row in &rows {
        let paras = &view.doc.main.paras;
        let mut p = table::row_props(paras, row, tw);
        if let Some(w) = width.filter(|w| *w >= 72) {
            for k in cols(p.cells.len()) {
                p.cells[k].width = w;
            }
        }
        for k in cols(p.cells.len()) {
            if let Some(v) = valign {
                p.cells[k].valign = v;
            }
            if let (Some(h), Some(&(a, b))) = (halign, row.cells.get(k)) {
                view.doc.format_paras(MAIN, a, b, |pap| pap.align = h);
            }
        }
        if let Some(g) = gap {
            p.gap = g.max(0);
        }
        if let Some(i) = indent {
            p.indent = i;
        }
        p.height = height;
        p.align = align;
        view.doc.set_row(MAIN, row.end, p);
    }
    let sel = view.sel;
    view.doc.commit(sel);
    true
}

// ----- Table Layout -----

pub fn table_layout(view: &DocView) -> Option<Dialog> {
    let ts = table_selection(view)?;
    let mut d = Dialog::new("Table Layout", 241.0, 130.0);
    d.radio("row", "Row", 16.0, 8.0, 1, false);
    d.radio("column", "Column", 16.0, 26.0, 1, false);
    d.radio("selection", "Selection", 16.0, 43.0, 1, true);
    d.group("Shift Cells", 14.0, 62.0, 111.0, 57.0);
    d.radio("horiz", "Horizontally", 21.0, 80.0, 2, true);
    d.radio("vert", "Vertically", 21.0, 98.0, 2, false);
    d.button("insert", "Insert", 141.0, 10.0, 84.0, false);
    d.button("delete", "Delete", 141.0, 33.0, 84.0, false);
    d.button("merge", "Merge Cells", 141.0, 56.0, 84.0, false);
    d.button("split", "Split Cell", 141.0, 79.0, 84.0, false);
    d.button("cancel", "Cancel", 141.0, 102.0, 84.0, false);
    if ts.from == ts.to {
        d.disable("merge");
    }
    Some(d)
}

/// Shift Cells only means something for Selection.
pub fn table_layout_changed(d: &mut Dialog) {
    let sel = d.is_on("selection");
    d.enable("horiz", sel);
    d.enable("vert", sel);
}

/// Insert, Delete, Merge or Split Cells in Table Layout.
pub fn table_layout_button(d: &Dialog, view: &mut DocView, id: &str) {
    let what = if d.is_on("row") || (d.is_on("selection") && d.is_on("vert")) {
        "row"
    } else if d.is_on("column") {
        "column"
    } else {
        "cells"
    };
    change_cells(view, id, what);
}

/// Change the selected cells, from Table Layout or a command: `id` is
/// insert (before them), after, delete, merge or split, and `what` is row,
/// column or cells.
pub fn change_cells(view: &mut DocView, id: &str, what: &str) {
    let Some(ts) = table_selection(view) else {
        return;
    };
    let tw = view.doc.column_width_at(ts.rows[0].first);
    let label = match id {
        "insert" | "after" => "Insert Cells",
        "delete" => "Delete Cells",
        "split" => "Split Cell",
        _ => "Merge Cells",
    };
    view.doc.begin(label, view.sel);
    let first_para = ts.rows[0].first;
    let paras = &view.doc.main.paras;
    let all_rows = table::table_rows(paras, first_para);
    let n = ts.to - ts.from + 1;
    if id == "delete" && view.doc.props.track_changes {
        // Track Changes keeps what is deleted, so Reject All can bring it
        // back: the text of the rows, columns or cells is marked deleted
        // and the table is left as it is.
        let rows = if what == "column" {
            all_rows
        } else {
            ts.rows.clone()
        };
        for row in &rows {
            let (from, to) = if what == "row" {
                (0, row.cells.len() - 1)
            } else {
                let from = ts.from.min(row.cells.len() - 1);
                (from, (from + n - 1).min(row.cells.len() - 1))
            };
            for &(a, b) in &row.cells[from..=to] {
                let end = view.doc.main.paras[b].len();
                view.doc
                    .delete(MAIN, Position::new(a, 0), Position::new(b, end));
            }
        }
        let sel = view.sel;
        view.doc.commit(sel);
        return;
    }
    match (id, what) {
        ("insert", "row") => {
            let row = &ts.rows[0];
            let props = table::row_props(paras, row, tw);
            view.doc
                .insert_rows(MAIN, row, false, ts.rows.len(), &props);
        }
        ("delete", "row") => {
            let (a, b) = (ts.rows[0].clone(), ts.rows.last().unwrap().clone());
            view.doc.delete_rows(MAIN, &a, &b);
        }
        ("split", _) => {
            for row in ts.rows.iter().rev() {
                let paras = &view.doc.main.paras;
                let p = table::row_props(paras, row, tw);
                view.doc.split_cells(MAIN, row, &p, ts.from, ts.to);
            }
        }
        ("merge", _) => {
            for row in ts.rows.iter().rev() {
                let paras = &view.doc.main.paras;
                let p = table::row_props(paras, row, tw);
                view.doc.merge_cells(MAIN, row, &p, ts.from, ts.to);
            }
        }
        (_, w) => {
            // Columns run through every row; cells just the selected ones.
            let rows = if w == "column" {
                all_rows
            } else {
                ts.rows.clone()
            };
            for row in rows.iter().rev() {
                let paras = &view.doc.main.paras;
                let p = table::row_props(paras, row, tw);
                let from = ts.from.min(row.cells.len() - 1);
                let to = (from + n - 1).min(row.cells.len() - 1);
                match id {
                    "insert" => {
                        let width = p.cells[from].width;
                        view.doc.insert_cells(MAIN, row, &p, from, n, width);
                    }
                    "after" => {
                        let width = p.cells[to].width;
                        view.doc.insert_cells(MAIN, row, &p, to + 1, n, width);
                    }
                    _ => view.doc.delete_cells(MAIN, row, &p, from, to),
                }
            }
        }
    }
    // Put the insertion point back in the table, where it was if possible.
    let paras = &view.doc.main.paras;
    let p = first_para.min(paras.len() - 1);
    view.set_selection(Selection::caret(Position::new(p, 0)));
    let sel = view.sel;
    view.doc.commit(sel);
}

/// Insert Row Above or Below: as many empty rows as are selected, above
/// the first or below the last. The insertion point goes to the first new
/// row, in the column it was in.
pub fn insert_rows(view: &mut DocView, below: bool) {
    let Some(ts) = table_selection(view) else {
        return;
    };
    let row = if below {
        ts.rows.last()
    } else {
        ts.rows.first()
    };
    let Some(row) = row.cloned() else {
        return;
    };
    let tw = view.doc.column_width_at(row.first);
    let props = table::row_props(&view.doc.main.paras, &row, tw);
    view.doc.begin("Insert Row", view.sel);
    view.doc
        .insert_rows(MAIN, &row, below, ts.rows.len(), &props);
    let at = if below { row.end + 1 } else { row.first };
    let new = table::row_at(&view.doc.main.paras, at);
    let p = new.map_or(at, |r| r.cells[ts.from.min(r.cells.len() - 1)].0);
    view.set_selection(Selection::caret(Position::new(p, 0)));
    let sel = view.sel;
    view.doc.commit(sel);
}

/// Insert Column Left or Right: as many empty columns as are selected,
/// through every row, each as wide as the cell beside it. A table that ran
/// margin to margin still does, its columns narrowed to make room.
pub fn insert_columns(view: &mut DocView, right: bool) {
    let Some(ts) = table_selection(view) else {
        return;
    };
    let first = ts.rows[0].first;
    let tw = view.doc.column_width_at(first);
    let here = view.sel.focus.para;
    let rows = table::table_rows(&view.doc.main.paras, first);
    let Some(top) = rows.first().map(|r| r.first) else {
        return;
    };
    let row_index = rows.iter().position(|r| r.first <= here && here <= r.end);
    let full = table::is_full_width(&view.doc.main.paras, &rows, tw);
    let n = ts.to - ts.from + 1;
    view.doc.begin("Insert Column", view.sel);
    for row in rows.iter().rev() {
        let p = table::row_props(&view.doc.main.paras, row, tw);
        let last = row.cells.len() - 1;
        let beside = if right {
            ts.to.min(last)
        } else {
            ts.from.min(last)
        };
        let at = if right { beside + 1 } else { beside };
        let width = p.cells[beside].width;
        view.doc.insert_cells(MAIN, row, &p, at, n, width);
    }
    let rows = table::table_rows(&view.doc.main.paras, top);
    if full {
        view.doc.fit_rows(MAIN, &rows, tw, tw);
    }
    let row = &rows[row_index.unwrap_or(0).min(rows.len() - 1)];
    let at = if right { ts.to + 1 } else { ts.from };
    let p = row.cells[at.min(row.cells.len() - 1)].0;
    view.set_selection(Selection::caret(Position::new(p, 0)));
    let sel = view.sel;
    view.doc.commit(sel);
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::Document;

    fn view_with_table() -> DocView {
        let mut v = DocView::new(Document::new(), "T".into());
        let at = v.doc.insert_table(MAIN, Position::new(0, 0), 2, 3, 2000);
        v.set_selection(Selection::caret(at));
        v
    }

    #[test]
    fn insert_row_above_and_below_span_the_table() {
        let mut v = view_with_table();
        let paras = &v.doc.main.paras;
        let second = table::table_rows(paras, 0)[1].cells[1].0;
        v.set_selection(Selection::caret(Position::new(second, 0)));
        insert_rows(&mut v, true);
        let rows = table::table_rows(&v.doc.main.paras, 0);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.cells.len() == 3));
        assert_eq!(v.sel.focus.para, rows[2].cells[1].0);
        insert_rows(&mut v, false);
        let rows = table::table_rows(&v.doc.main.paras, 0);
        assert_eq!(rows.len(), 4);
        assert_eq!(v.sel.focus.para, rows[2].cells[1].0);
        v.doc.undo();
        assert_eq!(table::table_rows(&v.doc.main.paras, 0).len(), 3);
    }

    #[test]
    fn insert_column_runs_through_every_row() {
        let mut v = view_with_table();
        insert_columns(&mut v, true);
        let paras = &v.doc.main.paras;
        let rows = table::table_rows(paras, 0);
        assert!(rows.iter().all(|r| r.cells.len() == 4));
        assert_eq!(v.sel.focus.para, rows[0].cells[1].0);
        let w = table::row_props(paras, &rows[1], 9360);
        assert_eq!(w.width(), 8000);
        insert_columns(&mut v, false);
        let rows = table::table_rows(&v.doc.main.paras, 0);
        assert!(rows.iter().all(|r| r.cells.len() == 5));
        assert_eq!(v.sel.focus.para, rows[0].cells[1].0);
    }

    #[test]
    fn a_full_width_table_stays_full_width_with_a_new_column() {
        let mut v = DocView::new(Document::new(), "T".into());
        let tw = v.doc.props.text_width();
        let at = v.doc.insert_table(MAIN, Position::new(0, 0), 2, 3, tw / 3);
        v.set_selection(Selection::caret(at));
        insert_columns(&mut v, false);
        let paras = &v.doc.main.paras;
        for r in table::table_rows(paras, 0) {
            let p = table::row_props(paras, &r, tw);
            assert_eq!(p.cells.len(), 4);
            assert_eq!(p.width(), tw);
        }
    }

    #[test]
    fn insert_table_dialog_makes_a_table() {
        let mut v = DocView::new(Document::new(), "T".into());
        let (mut d, st) = insert_table(&v);
        assert_eq!(d.field_text("width"), "3 in");
        assert!(!d.is_enabled("c_tabs"));
        d.set_field("cols", "4");
        insert_changed(&v, &mut d, "cols");
        assert_eq!(d.field_text("width"), "1.5 in");
        assert!(insert_apply(&st, &d, &mut v));
        let row = table::row_at(&v.doc.main.paras, 0).unwrap();
        assert_eq!(row.cells.len(), 4);
        assert_eq!(v.sel.focus, Position::new(0, 0));
    }

    #[test]
    fn insert_table_converts_text_wider_than_the_column_limit() {
        let line = (1..=70)
            .map(|n| format!("c{n}"))
            .collect::<Vec<_>>()
            .join("\t");
        let text = format!("{line}\n{line}");
        let mut v = DocView::new(quill_core::text::from_plain_text(&text), "T".into());
        let end = v.doc.main.paras[1].len();
        v.set_selection(Selection::new(Position::new(0, 0), Position::new(1, end)));
        let (d, st) = insert_table(&v);
        assert_eq!(d.field_text("cols"), "70");
        assert!(insert_apply(&st, &d, &mut v));
        let rows = table::table_rows(&v.doc.main.paras, 0);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.cells.len() == 70));
    }

    #[test]
    fn insert_table_refuses_a_new_table_with_too_many_columns() {
        let mut v = DocView::new(Document::new(), "T".into());
        let (mut d, st) = insert_table(&v);
        d.set_field("cols", "32");
        assert!(insert_values(&st, &d).is_err());
        assert!(!insert_apply(&st, &d, &mut v));
        assert!(table::row_at(&v.doc.main.paras, 0).is_none());
    }

    #[test]
    fn convert_to_table_offers_the_lines_that_match() {
        let text = "intro\na,b,c\nd,e,f\ng,h,i\nj,k,l\nclosing, with a comma";
        let mut v = DocView::new(quill_core::text::from_plain_text(text), "T".into());
        let end = v.doc.main.paras[3].len();
        v.set_selection(Selection::new(Position::new(2, 0), Position::new(3, end)));
        let plan = convert_plan(&v).unwrap();
        assert_eq!(plan.how, Convert::Commas);
        assert_eq!((plan.first, plan.last), (2, 3));
        assert_eq!((plan.above, plan.below), (1, 1));
        convert_lines(&mut v, &plan, true);
        let rows = table::table_rows(&v.doc.main.paras, 1);
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|r| r.cells.len() == 3));
        assert_eq!(v.doc.main.paras[0].text, "intro");
        assert!(
            v.doc
                .main
                .paras
                .iter()
                .any(|p| p.text.starts_with("closing"))
        );
    }

    #[test]
    fn convert_to_table_selection_only_and_no_plan_for_plain_text() {
        let mut v = DocView::new(
            quill_core::text::from_plain_text("a\tb\nc\td\ne\tf"),
            "T".into(),
        );
        let end = v.doc.main.paras[1].len();
        v.set_selection(Selection::new(Position::new(1, 0), Position::new(1, end)));
        let plan = convert_plan(&v).unwrap();
        assert_eq!((plan.above, plan.below), (1, 1));
        convert_lines(&mut v, &plan, false);
        assert_eq!(table::table_rows(&v.doc.main.paras, 0).len(), 0);
        assert_eq!(table::table_rows(&v.doc.main.paras, 1).len(), 1);
        let mut plain = DocView::new(
            quill_core::text::from_plain_text("no delimiters"),
            "T".into(),
        );
        plain.set_selection(Selection::new(Position::new(0, 0), Position::new(0, 5)));
        assert!(convert_plan(&plain).is_none());
    }

    #[test]
    fn insert_table_in_a_cell_makes_a_table_in_the_cell() {
        let mut v = view_with_table();
        let (mut d, st) = insert_table(&v);
        d.set_field("rows", "2");
        d.set_field("cols", "2");
        insert_changed(&v, &mut d, "cols");
        // Each column gets half of what the cell has, less its padding.
        assert_eq!(
            d.field_text("width"),
            units::length((2000 - quill_core::props::TABLE_GAP) / 2)
        );
        assert!(insert_apply(&st, &d, &mut v));
        let paras = &v.doc.main.paras;
        assert_eq!(paras.iter().filter(|p| p.pap.level() == 2).count(), 6);
        let inner = table::table_rows(paras, v.sel.focus.para);
        assert_eq!(inner.len(), 2);
        assert!(inner.iter().all(|r| r.cells.len() == 2));
        // The outer table is still two rows by three cells.
        let outer = table::row_at_level(paras, 0, 1).unwrap();
        assert_eq!(outer.cells.len(), 3);
        // Tab moves between the inner cells, then to a new inner row.
        v.sel = Selection::caret(Position::new(inner[1].cells[1].0, 0));
        let mut clip = crate::docview::Clipboard::new();
        v.exec(crate::commands::Cmd::Tab, false, &mut clip);
        let rows = table::table_rows(&v.doc.main.paras, v.sel.focus.para);
        assert_eq!(rows.len(), 3);
        assert_eq!(v.doc.main.paras[v.sel.focus.para].pap.level(), 2);
    }

    #[test]
    fn a_table_inserted_in_a_column_is_as_wide_as_the_column() {
        let mut v = DocView::new(Document::new(), "T".into());
        v.doc.section.columns = 2;
        let column = v.doc.section.column_width(v.doc.props.text_width());
        assert!(column < v.doc.props.text_width() / 2);
        let (mut d, st) = insert_table(&v);
        d.set_field("cols", "3");
        insert_changed(&v, &mut d, "cols");
        let width = units::parse_length(&d.field_text("width")).unwrap();
        assert!((width * 3 - column).abs() <= 20, "{width} x 3 in {column}");
        // A table the width of the page is too wide for the column.
        let (_, room) = insert_too_wide(&st, &d, &v, 3, column).unwrap();
        assert_eq!(room, column);
    }

    #[test]
    fn insert_table_notices_columns_wider_than_the_room() {
        let v = DocView::new(Document::new(), "T".into());
        let (mut d, st) = insert_table(&v);
        // 4 columns of 3 inches on a 6 inch column: too wide.
        d.set_field("cols", "4");
        d.set_field("width", "3 in");
        let (total, room) = insert_too_wide(&st, &d, &v, 4, 4320).unwrap();
        assert_eq!((total, room), (17280, v.doc.props.text_width()));
        // The width the dialog fills in fits.
        insert_changed(&v, &mut d, "cols");
        let width = units::parse_length(&d.field_text("width")).unwrap();
        assert!(insert_too_wide(&st, &d, &v, 4, width).is_none());
        // In a cell the room is the cell's.
        let v = view_with_table();
        let (d, st) = insert_table(&v);
        let (_, room) = insert_too_wide(&st, &d, &v, 2, 2000).unwrap();
        assert_eq!(room, 2000 - quill_core::props::TABLE_GAP);
    }

    #[test]
    fn insert_table_converts_selected_text() {
        let mut v = DocView::new(quill_core::text::from_plain_text("a\tb\nc\td"), "T".into());
        v.set_selection(Selection::new(Position::new(0, 0), Position::new(1, 3)));
        let (d, st) = insert_table(&v);
        assert_eq!(d.field_text("cols"), "2");
        assert!(d.is_on("c_tabs"));
        assert!(insert_apply(&st, &d, &mut v));
        let rows = table::table_rows(&v.doc.main.paras, 0);
        assert_eq!(rows.len(), 2);
        assert_eq!(v.doc.main.paras[rows[1].cells[1].0].text, "d");
    }

    #[test]
    fn table_cells_sets_widths_and_row_settings() {
        let mut v = view_with_table();
        let mut d = table_cells(&v).unwrap();
        assert_eq!(d.field_text("width"), "1.39 in");
        d.set_field("width", "2 in");
        d.set_selected("align", 1);
        d.set_selected("height", 2);
        d.set_field("height_by", "30 pt");
        assert!(table_cells_apply(&d, &mut v));
        let paras = &v.doc.main.paras;
        let row = table::row_at(paras, 0).unwrap();
        let p = table::row_props(paras, &row, 8640);
        assert_eq!(p.cells[0].width, 2880);
        assert_eq!(p.cells[1].width, 2000);
        assert_eq!(p.align, Align::Center);
        assert_eq!(p.height, RowHeight::Exactly(600));
        // Each Cell In Table reaches the second row too.
        d.set_selected("applyto", 1);
        assert!(table_cells_apply(&d, &mut v));
        let paras = &v.doc.main.paras;
        let r2 = table::row_at(paras, row.end + 1).unwrap();
        let p2 = table::row_props(paras, &r2, 8640);
        assert!(p2.cells.iter().all(|c| c.width == 2880));
    }

    #[test]
    fn a_new_table_has_borders_from_the_settings() {
        let mut v = DocView::new(Document::new(), String::new());
        v.doc.begin("t", v.sel);
        let at = v.doc.insert_table(MAIN, Position::new(0, 0), 2, 2, 2000);
        border_new_table(&mut v, at);
        let sel = v.sel;
        v.doc.commit(sel);
        let line = new_table_line();
        for row in table::table_rows(&v.doc.main.paras, 0) {
            let p = table::row_props(&v.doc.main.paras, &row, 8640);
            assert!(p.cells.iter().all(|c| {
                let b = c.borders;
                [b.top, b.bottom, b.left, b.right] == [line; 4]
            }));
        }
        // Changing the setting later leaves the table as it is: it holds
        // its own color and style.
        assert_ne!(line.style, BorderStyle::None);
    }

    #[test]
    fn cells_go_in_before_or_after_the_one_selected() {
        let mut v = DocView::new(Document::new(), String::new());
        v.doc.begin("t", v.sel);
        v.doc.insert_table(MAIN, Position::new(0, 0), 1, 2, 2000);
        let sel = v.sel;
        v.doc.commit(sel);
        v.set_selection(Selection::caret(Position::new(0, 0)));
        v.type_text("A");
        let cell = |v: &DocView| {
            let row = table::row_at(&v.doc.main.paras, 0).unwrap();
            let k = row
                .cells
                .iter()
                .position(|&(a, _)| v.doc.main.paras[a].text == "A")
                .unwrap();
            (k, row.cells.len())
        };
        change_cells(&mut v, "after", "cells");
        assert_eq!(cell(&v), (0, 3));
        v.set_selection(Selection::caret(Position::new(0, 0)));
        change_cells(&mut v, "insert", "cells");
        assert_eq!(cell(&v), (1, 4));
    }

    #[test]
    fn table_layout_inserts_and_deletes() {
        let mut v = view_with_table();
        let mut d = table_layout(&v).unwrap();
        d.set_on("selection", false);
        d.set_on("row", true);
        table_layout_button(&d, &mut v, "insert");
        assert_eq!(table::table_rows(&v.doc.main.paras, 0).len(), 3);
        d.set_on("row", false);
        d.set_on("column", true);
        table_layout_button(&d, &mut v, "delete");
        let rows = table::table_rows(&v.doc.main.paras, 0);
        assert!(rows.iter().all(|r| r.cells.len() == 2));
        v.doc.undo();
        let rows = table::table_rows(&v.doc.main.paras, 0);
        assert!(rows.iter().all(|r| r.cells.len() == 3));
    }

    #[test]
    fn table_layout_delete_is_tracked_and_rejected() {
        let mut v = view_with_table();
        let chp = v.doc.styles.get(0).chp;
        let cells: Vec<usize> = table::table_rows(&v.doc.main.paras, 0)
            .iter()
            .flat_map(|r| r.cells.iter().map(|c| c.0))
            .collect();
        for (i, &c) in cells.iter().enumerate() {
            v.doc.main.paras[c].insert(0, &format!("c{i}"), chp);
        }
        v.doc.forget_history();
        v.doc.props.track_changes = true;
        v.doc.author = "Ada".into();
        let mut d = table_layout(&v).unwrap();
        d.set_on("selection", false);
        d.set_on("row", true);
        table_layout_button(&d, &mut v, "delete");
        // The table is still there, its first row's text marked deleted.
        let rows = table::table_rows(&v.doc.main.paras, 0);
        assert_eq!(rows.len(), 2);
        assert!(v.doc.has_changes());
        assert_eq!(v.doc.main.paras[cells[0]].text, "c0");
        v.doc.resolve_all_changes(false);
        assert!(!v.doc.has_changes());
        assert_eq!(table::table_rows(&v.doc.main.paras, 0).len(), 2);
        assert_eq!(v.doc.main.paras[cells[0]].text, "c0");
        // Accepting it empties the cells.
        table_layout_button(&d, &mut v, "delete");
        v.doc.resolve_all_changes(true);
        assert_eq!(v.doc.main.paras[cells[0]].text, "");
        assert_eq!(v.doc.main.paras[cells[3]].text, "c3");
    }

    #[test]
    fn table_layout_splits_the_selected_cell_in_two() {
        let mut v = view_with_table();
        let chp = v.doc.styles.get(0).chp;
        let first = table::table_rows(&v.doc.main.paras, 0)[0].cells[0].0;
        v.doc.main.paras[first].insert(0, "keep", chp);
        v.doc.forget_history();
        v.set_selection(Selection::caret(Position::new(first, 0)));
        let d = table_layout(&v).unwrap();
        table_layout_button(&d, &mut v, "split");
        let paras = &v.doc.main.paras;
        let rows = table::table_rows(paras, 0);
        assert_eq!(rows[0].cells.len(), 4);
        assert_eq!(rows[1].cells.len(), 3);
        let p = table::row_props(paras, &rows[0], 6000);
        assert_eq!(
            p.cells.iter().map(|c| c.width).collect::<Vec<_>>(),
            [1000, 1000, 2000, 2000]
        );
        assert_eq!(paras[rows[0].cells[0].0].text, "keep");
        assert_eq!(paras[rows[0].cells[1].0].text, "");
        v.doc.undo();
        assert_eq!(table::table_rows(&v.doc.main.paras, 0)[0].cells.len(), 3);
    }
}
