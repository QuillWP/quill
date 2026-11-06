//! Insert > Table, Format > Table Cells and Format > Table Layout. Layouts
//! measured from the original.

use crate::dialog::{Dialog, units};
use crate::docview::DocView;
use quill_core::props::{Align, RowHeight, RowProps, Twips};
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
    let width = view.doc.props.text_width() / cols as Twips;
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
        let w = view.doc.props.text_width() / n as Twips;
        d.set_field("width", &units::length(w));
    }
}

/// OK (or Format…): insert the table. False if a field is bad.
pub fn insert_apply(st: &InsertState, d: &Dialog, view: &mut DocView) -> bool {
    let cols: usize = match d.field_text("cols").trim().parse() {
        Ok(n) if (1..=31).contains(&n) => n,
        _ => return false,
    };
    let rows: usize = match d.field_text("rows").trim().parse() {
        Ok(n) if (1..=999).contains(&n) => n,
        _ => return false,
    };
    let Some(width) = units::parse_length(&d.field_text("width")).filter(|w| *w >= 360) else {
        return false;
    };
    view.doc.begin("Insert Table", view.sel);
    let at = match st.convert {
        Some((a, b)) => {
            let how = match d.radio_on(1) {
                "c_tabs" => Convert::Tabs,
                "c_commas" => Convert::Commas,
                "c_side" => Convert::SideBySide,
                _ => Convert::Paragraphs(cols),
            };
            view.doc.convert_to_table(MAIN, a, b, how, |_| width)
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
    view.set_selection(Selection::caret(at));
    let sel = view.sel;
    view.doc.commit(sel);
    true
}

// ----- Table Cells -----

const APPLY_TO: [&str; 4] = [
    "Selection",
    "Each Cell In Table",
    "Entire Columns Selected",
    "Entire Rows Selected",
];

pub fn table_cells(view: &DocView) -> Option<Dialog> {
    let ts = table_selection(view)?;
    let paras = &view.doc.main.paras;
    let tw = view.doc.props.text_width();
    let props: Vec<RowProps> = ts
        .rows
        .iter()
        .map(|r| table::row_props(paras, r, tw))
        .collect();
    // The column width, if the selected cells share one.
    let widths: Vec<Twips> = props
        .iter()
        .flat_map(|p| {
            p.cells[ts.from.min(p.cells.len() - 1)..=ts.to.min(p.cells.len() - 1)]
                .iter()
                .map(|c| c.width)
        })
        .collect();
    let width = if widths.windows(2).all(|w| w[0] == w[1]) {
        widths
            .first()
            .map(|w| units::length(*w))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let p = &props[0];
    let mut d = Dialog::new("Table Cells", 352.0, 182.0);
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
    let len = |id| {
        let t = d.field_text(id);
        if t.trim().is_empty() {
            Ok(None)
        } else {
            units::parse_length(&t).map(Some).ok_or(())
        }
    };
    let (Ok(width), Ok(gap), Ok(indent)) = (len("width"), len("gap"), len("indent")) else {
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
    let tw = view.doc.props.text_width();
    view.doc.begin("Format Cells", view.sel);
    for row in &rows {
        let paras = &view.doc.main.paras;
        let mut p = table::row_props(paras, row, tw);
        if let Some(w) = width.filter(|w| *w >= 72) {
            for k in cols(p.cells.len()) {
                p.cells[k].width = w;
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
    d.button("cancel", "Cancel", 141.0, 79.0, 84.0, false);
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

/// Insert, Delete or Merge Cells in Table Layout.
pub fn table_layout_button(d: &Dialog, view: &mut DocView, id: &str) {
    let Some(ts) = table_selection(view) else {
        return;
    };
    let tw = view.doc.props.text_width();
    let what = if d.is_on("row") || (d.is_on("selection") && d.is_on("vert")) {
        "row"
    } else if d.is_on("column") {
        "column"
    } else {
        "cells"
    };
    let label = match id {
        "insert" => "Insert Cells",
        "delete" => "Delete Cells",
        _ => "Merge Cells",
    };
    view.doc.begin(label, view.sel);
    let first_para = ts.rows[0].first;
    let paras = &view.doc.main.paras;
    let all_rows = table::table_rows(paras, first_para);
    let n = ts.to - ts.from + 1;
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
                if id == "insert" {
                    let width = p.cells[from].width;
                    view.doc.insert_cells(MAIN, row, &p, from, n, width);
                } else {
                    let to = (from + n - 1).min(row.cells.len() - 1);
                    view.doc.delete_cells(MAIN, row, &p, from, to);
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
}
