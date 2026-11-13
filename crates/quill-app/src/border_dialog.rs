//! Format > Border: the sides to draw, a line style and color, presets,
//! distance from text and a background color, for a paragraph, cells or a whole table.

use crate::dialog::{Dialog, units};
use crate::docview::DocView;
use quill_core::props::{BorderLine, BorderStyle, Borders, Rgb};

/// Sides of the box, as bits of `BorderState::sel`.
const TOP: u8 = 1;
const BOTTOM: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;
const BETWEEN: u8 = 16;
const BOX: u8 = TOP | BOTTOM | LEFT | RIGHT;

/// The line list, top to bottom.
const LINES: [(&str, BorderStyle); 6] = [
    ("None", BorderStyle::None),
    ("Dotted", BorderStyle::Dotted),
    ("Hairline", BorderStyle::Hairline),
    ("Single", BorderStyle::Single),
    ("Thick", BorderStyle::Thick),
    ("Double", BorderStyle::Double),
];
#[derive(Clone, Debug)]
pub struct BorderState {
    /// What the paragraph had, to tell whether anything changed.
    base: (Borders, u16, Option<Rgb>),
    pub borders: Borders,
    pub shading: u16,
    pub shading_color: Option<Rgb>,
    /// Sides ticked in the dialog.
    pub sel: u8,
    /// The line style chosen in the list.
    pub line: BorderStyle,
    /// The line color chosen.
    pub color: Rgb,
}

fn side(b: &mut Borders, bit: u8) -> &mut BorderLine {
    match bit {
        TOP => &mut b.top,
        BOTTOM => &mut b.bottom,
        LEFT => &mut b.left,
        RIGHT => &mut b.right,
        _ => &mut b.between,
    }
}

/// Show the line color and the background in their boxes, with the HTML
/// code beside the line color.
fn show_colors(st: &BorderState, d: &mut Dialog) {
    d.set_swatch("line_color", st.color.0);
    d.set_label("line_color_hex", &crate::palette::hex(st.color));
    d.set_swatch("shading_color", st.shading_color.map_or(0xFFFFFF, |c| c.0));
}

/// The color the palette starts on for color box `anchor`.
pub fn color_start(st: &BorderState, anchor: &str) -> Option<Rgb> {
    match anchor {
        "line_color" => Some(st.color),
        _ => st.shading_color,
    }
}

/// Color box `anchor` was given `color` (None puts the background away).
/// A new line color goes to the sides picked. The background is a plain
/// color: it replaces any gray percentage the paragraph had.
pub fn set_color(st: &mut BorderState, d: &mut Dialog, anchor: &str, color: Option<Rgb>) {
    if anchor == "line_color" {
        st.color = color.unwrap_or(Rgb::BLACK);
        for bit in [TOP, BOTTOM, LEFT, RIGHT, BETWEEN] {
            if st.sel & bit != 0 {
                side(&mut st.borders, bit).color = st.color;
            }
        }
    } else {
        st.shading_color = color;
        st.shading = 0;
    }
    show_colors(st, d);
    update_preview(st, d);
}

/// Refresh controls after choosing a different target for the formatting.
fn sync_controls(st: &BorderState, d: &mut Dialog) {
    show_colors(st, d);
    let space = [
        st.borders.top,
        st.borders.left,
        st.borders.bottom,
        st.borders.right,
    ]
    .iter()
    .map(|l| l.space)
    .max()
    .unwrap_or(0);
    d.set_field("space", &units::points(space));
    for (id, bit) in [
        ("top", TOP),
        ("bottom", BOTTOM),
        ("left", LEFT),
        ("right", RIGHT),
        ("between", BETWEEN),
    ] {
        d.set_on(id, !side(&mut st.borders.clone(), bit).is_none());
    }
    d.set_selected(
        "lines",
        LINES
            .iter()
            .position(|(_, style)| *style == st.line)
            .unwrap_or(3),
    );
    update_preview(st, d);
}

/// Apply To: Paragraph (0), Cells (1) or Table (2) was chosen. What the
/// person already changed in the dialog goes with them to the new target;
/// otherwise the new target shows its own borders and shading.
pub fn target_changed(st: &mut BorderState, d: &mut Dialog, view: &DocView) {
    let loaded = load_target(st.clone(), view, d.selected("applyto"));
    let edited = (st.borders, st.shading, st.shading_color) != st.base;
    if edited {
        st.base = loaded.base;
    } else {
        *st = loaded;
    }
    sync_controls(st, d);
}

/// The dialog opened on Apply To `target` (1 cells, 2 table) when in a table.
pub fn border_at(view: &DocView, target: usize) -> (Dialog, BorderState) {
    let (mut d, st) = border(view);
    if crate::table_dialogs::table_selection(view).is_none() {
        return (d, st);
    }
    d.set_selected("applyto", target);
    let st = for_cells(st, view, &mut d);
    (d, st)
}

pub fn border(view: &DocView) -> (Dialog, BorderState) {
    let p = view.current_pap();
    let b = p.borders;
    let mut sel = 0;
    let mut line = BorderStyle::Single;
    for bit in [TOP, BOTTOM, LEFT, RIGHT, BETWEEN] {
        let l = *side(&mut b.clone(), bit);
        if !l.is_none() {
            if sel == 0 {
                line = l.style;
            }
            sel |= bit;
        }
    }
    let space = [b.top, b.left, b.bottom, b.right]
        .iter()
        .map(|l| l.space)
        .max()
        .unwrap_or(0);
    // In a table the cell's own lines say which color to start from.
    let cell = crate::table_dialogs::table_selection(view).map(|ts| {
        let paras = &view.doc.main.paras;
        let row = quill_core::table::row_props(paras, &ts.rows[0], view.doc.props.text_width());
        row.cells[ts.from.min(row.cells.len() - 1)].borders
    });
    let color = [cell, Some(b)]
        .into_iter()
        .flatten()
        .flat_map(|b| [b.top, b.bottom, b.left, b.right, b.between])
        .find(|l| !l.is_none())
        .map_or(Rgb::BLACK, |l| l.color);
    let mut d = Dialog::new("Border", 368.0, 248.0);
    d.group("Border", 12.0, 6.0, 149.0, 161.0);
    d.check("top", "Top", 18.0, 24.0, sel & TOP != 0);
    d.check("bottom", "Bottom", 18.0, 46.0, sel & BOTTOM != 0);
    d.check("left", "Left", 18.0, 68.0, sel & LEFT != 0);
    d.check("right", "Right", 18.0, 90.0, sel & RIGHT != 0);
    d.check("between", "Between", 18.0, 112.0, sel & BETWEEN != 0);
    d.text("From Text:", 16.0, 144.0);
    d.field("space", &units::points(space), 89.0, 142.0, 68.0);
    d.group("Preset Borders", 12.0, 170.0, 149.0, 68.0);
    d.radio("p_none", "None", 18.0, 187.0, 2, b.is_none());
    d.radio("p_box", "Box", 18.0, 207.0, 2, !b.is_none() && !b.shadow);
    d.radio("p_shadow", "Shadow", 78.0, 207.0, 2, b.shadow);
    d.group("Line", 169.0, 6.0, 114.0, 130.0);
    let line_names: Vec<&str> = LINES.iter().map(|(name, _)| *name).collect();
    let line_at = LINES
        .iter()
        .position(|(_, style)| *style == line)
        .unwrap_or(3);
    d.popup("lines", &line_names, line_at, 174.0, 25.0, 104.0);
    d.text_as("line_preview", "", 176.0, 58.0);
    d.text("Color:", 176.0, 80.0);
    d.swatch_button("line_color", 174.0, 97.0, 44.0, 22.0, color.0);
    d.text_as("line_color_hex", &crate::palette::hex(color), 224.0, 100.0);
    d.button("ok", "OK", 304.0, 15.0, 52.0, true);
    d.button("cancel", "Cancel", 304.0, 45.0, 52.0, false);
    d.text("Background:", 169.0, 160.0);
    d.swatch_button(
        "shading_color",
        169.0,
        177.0,
        40.0,
        22.0,
        p.shading_color.map_or(0xFFFFFF, |c| c.0),
    );
    d.button("shading_clear", "No Color", 215.0, 178.0, 58.0, false);
    d.text_as("applyto_label", "Apply To:", 169.0, 203.0);
    d.popup(
        "applyto",
        &["Paragraph", "Cells", "Table"],
        0,
        169.0,
        217.0,
        186.0,
    );
    // Apply To: Cells and Table only mean something in a table.
    if crate::table_dialogs::table_selection(view).is_none() {
        d.disable("applyto_label");
        d.disable("applyto");
    }
    d.focus("space");
    let st = BorderState {
        base: (b, p.shading, p.shading_color),
        borders: b,
        shading: p.shading,
        shading_color: p.shading_color,
        sel,
        line,
        color,
    };
    update_preview(&st, &mut d);
    (d, st)
}

/// The cells `target` (1 cells, 2 table) puts borders on: the selected
/// cells, or every cell of the table the insertion point is in.
fn target_cells(view: &DocView, target: usize) -> Option<crate::table_dialogs::TableSel> {
    let mut ts = crate::table_dialogs::table_selection(view)?;
    if target == 2 {
        let paras = &view.doc.main.paras;
        // A selected table ends at the start of the paragraph after it, so
        // the table is found from where the selection starts.
        ts.rows = quill_core::table::table_rows(paras, view.sel.start().para);
        ts.from = 0;
        ts.to = usize::MAX;
    }
    (!ts.rows.is_empty()).then_some(ts)
}

/// The borders and shading `target` has now, as the dialog's starting point.
fn load_target(mut st: BorderState, view: &DocView, target: usize) -> BorderState {
    if target == 0 {
        return border(view).1;
    }
    if let Some(ts) = target_cells(view, target) {
        let paras = &view.doc.main.paras;
        let p = quill_core::table::row_props(paras, &ts.rows[0], view.doc.props.text_width());
        let cell = &p.cells[ts.from.min(p.cells.len() - 1)];
        st.borders = cell.borders;
        st.shading = cell.shading;
        st.shading_color = cell.shading_color;
        st.base = (cell.borders, cell.shading, cell.shading_color);
        st.sel = 0;
        for bit in [TOP, BOTTOM, LEFT, RIGHT, BETWEEN] {
            let line = *side(&mut st.borders.clone(), bit);
            if !line.is_none() {
                if st.sel == 0 {
                    st.line = line.style;
                    st.color = line.color;
                }
                st.sel |= bit;
            }
        }
    }
    st
}

/// Start from the borders and shading of the target chosen in Apply To.
pub fn for_cells(st: BorderState, view: &DocView, d: &mut Dialog) -> BorderState {
    let st = load_target(st, view, d.selected("applyto"));
    sync_controls(&st, d);
    st
}

/// Put borders and shading on the selected cells.
fn apply_to_cells(
    b: Borders,
    base: Borders,
    (shading, shading_color): (u16, Option<Rgb>),
    borders_changed: bool,
    shading_changed: bool,
    target: usize,
    view: &mut DocView,
) {
    let Some(ts) = target_cells(view, target) else {
        return;
    };
    let tw = view.doc.props.text_width();
    view.doc.begin("Border", view.sel);
    for row in &ts.rows {
        let mut p = quill_core::table::row_props(&view.doc.main.paras, row, tw);
        let n = p.cells.len();
        for k in ts.from.min(n - 1)..=ts.to.min(n - 1) {
            if borders_changed {
                p.cells[k].borders = b;
            }
            if shading_changed {
                p.cells[k].shading = shading;
                p.cells[k].shading_color = shading_color;
            }
        }
        view.doc.set_row(quill_core::StoryId::Main, row.end, p);
    }
    if borders_changed {
        match_neighbors(view, &ts, &b, &base);
    }
    let sel = view.sel;
    view.doc.commit(sel);
}

/// Two cells that touch both draw their own line on the shared edge, and the
/// one drawn last wins. So a side that changed is also set on the cell
/// across it, outside the cells being changed, to keep the edge one color.
fn match_neighbors(
    view: &mut DocView,
    ts: &crate::table_dialogs::TableSel,
    b: &Borders,
    base: &Borders,
) {
    use quill_core::StoryId;
    let tw = view.doc.props.text_width();
    let all = quill_core::table::table_rows(&view.doc.main.paras, ts.rows[0].first);
    let same = |x: BorderLine, y: BorderLine| (x.style, x.color) == (y.style, y.color);
    let copy = |to: &mut BorderLine, from: BorderLine| {
        to.style = from.style;
        to.color = from.color;
    };
    // Where each cell of a row starts and ends across the table.
    let spans = |p: &quill_core::props::RowProps| -> Vec<(i32, i32)> {
        let mut x = 0;
        p.cells
            .iter()
            .map(|c| {
                let span = (x, x + c.width);
                x = span.1;
                span
            })
            .collect()
    };
    let first = all.iter().position(|r| r.first == ts.rows[0].first);
    let last = all
        .iter()
        .position(|r| r.first == ts.rows[ts.rows.len() - 1].first);
    let (Some(first), Some(last)) = (first, last) else {
        return;
    };
    // Left and right of each row's block.
    for row in &ts.rows {
        let mut p = quill_core::table::row_props(&view.doc.main.paras, row, tw);
        let n = p.cells.len();
        let (from, to) = (ts.from.min(n - 1), ts.to.min(n - 1));
        let mut edited = false;
        if from > 0 && !same(b.left, base.left) {
            copy(&mut p.cells[from - 1].borders.right, b.left);
            edited = true;
        }
        if to + 1 < n && !same(b.right, base.right) {
            copy(&mut p.cells[to + 1].borders.left, b.right);
            edited = true;
        }
        if edited {
            view.doc.set_row(StoryId::Main, row.end, p);
        }
    }
    // Top of the first row and bottom of the last: the cells across them
    // that share any of the block's width.
    let own = quill_core::table::row_props(&view.doc.main.paras, &all[first], tw);
    let n = own.cells.len();
    let spans_own = spans(&own);
    let (x0, x1) = (
        spans_own[ts.from.min(n - 1)].0,
        spans_own[ts.to.min(n - 1)].1,
    );
    for (edge, row, line) in [
        (0, first.checked_sub(1), b.top),
        (1, Some(last + 1).filter(|&r| r < all.len()), b.bottom),
    ] {
        let unchanged = if edge == 0 {
            same(b.top, base.top)
        } else {
            same(b.bottom, base.bottom)
        };
        let Some(r) = row.filter(|_| !unchanged) else {
            continue;
        };
        let mut p = quill_core::table::row_props(&view.doc.main.paras, &all[r], tw);
        for (k, (a, z)) in spans(&p).into_iter().enumerate() {
            if a < x1 && z > x0 {
                let side = if edge == 0 {
                    &mut p.cells[k].borders.bottom
                } else {
                    &mut p.cells[k].borders.top
                };
                copy(side, line);
            }
        }
        view.doc.set_row(StoryId::Main, all[r].end, p);
    }
}

/// Keep the standard controls and the live native preview in step.
pub fn changed(st: &mut BorderState, d: &mut Dialog, id: &str) {
    match id {
        "lines" => {
            st.line = LINES[d.selected("lines").min(LINES.len() - 1)].1;
            if st.sel == 0 {
                st.sel = BOX;
                for (control, _) in [
                    ("top", TOP),
                    ("bottom", BOTTOM),
                    ("left", LEFT),
                    ("right", RIGHT),
                ] {
                    d.set_on(control, true);
                }
            }
            for bit in [TOP, BOTTOM, LEFT, RIGHT, BETWEEN] {
                if st.sel & bit != 0 {
                    let line = side(&mut st.borders, bit);
                    line.style = st.line;
                    line.color = st.color;
                }
            }
            if st.borders.is_none() {
                st.borders.shadow = false;
            }
        }
        "top" | "bottom" | "left" | "right" | "between" => {
            st.sel = 0;
            for (control, bit) in [
                ("top", TOP),
                ("bottom", BOTTOM),
                ("left", LEFT),
                ("right", RIGHT),
                ("between", BETWEEN),
            ] {
                let line = side(&mut st.borders, bit);
                if d.is_on(control) {
                    st.sel |= bit;
                    line.style = if st.line == BorderStyle::None {
                        BorderStyle::Single
                    } else {
                        st.line
                    };
                    line.color = st.color;
                } else {
                    line.style = BorderStyle::None;
                }
            }
            st.borders.shadow &= st.sel & BOX == BOX;
        }
        "p_none" => set_preset(st, d, false, false),
        "p_box" => set_preset(st, d, true, false),
        "p_shadow" => set_preset(st, d, true, true),
        _ => {}
    }
    update_preview(st, d);
}

fn set_preset(st: &mut BorderState, d: &mut Dialog, boxed: bool, shadow: bool) {
    if st.line == BorderStyle::None {
        st.line = BorderStyle::Single;
        d.set_selected("lines", 3);
    }
    st.borders = Borders::default();
    st.sel = if boxed { BOX } else { 0 };
    for (control, bit) in [
        ("top", TOP),
        ("bottom", BOTTOM),
        ("left", LEFT),
        ("right", RIGHT),
        ("between", BETWEEN),
    ] {
        let on = boxed && bit != BETWEEN;
        d.set_on(control, on);
        if on {
            let line = side(&mut st.borders, bit);
            line.style = st.line;
            line.color = st.color;
        }
    }
    st.borders.shadow = shadow;
}

fn update_preview(st: &BorderState, d: &mut Dialog) {
    let line_name = LINES
        .iter()
        .find(|(_, style)| *style == st.line)
        .map_or("None", |(name, _)| *name);
    d.set_label("line_preview", &format!("{line_name} border"));
    let boxed = [
        st.borders.top,
        st.borders.bottom,
        st.borders.left,
        st.borders.right,
    ]
    .iter()
    .all(|line| !line.is_none());
    d.set_on("p_none", st.borders.is_none());
    d.set_on("p_box", boxed && !st.borders.shadow);
    d.set_on("p_shadow", boxed && st.borders.shadow);
}

/// OK: put the borders and shading on the selected paragraphs. False if
/// From Text is not a measurement.
pub fn apply(st: &BorderState, d: &Dialog, view: &mut DocView) -> bool {
    let old_space = [
        st.base.0.top,
        st.base.0.left,
        st.base.0.bottom,
        st.base.0.right,
    ]
    .iter()
    .map(|l| l.space)
    .max()
    .unwrap_or(0);
    let Some(space) = units::parse_length_from(&d.field_text("space"), old_space) else {
        return false;
    };
    let mut b = st.borders;
    for bit in [TOP, BOTTOM, LEFT, RIGHT, BETWEEN] {
        let old = *side(&mut st.base.0.clone(), bit);
        let l = side(&mut b, bit);
        if space != old_space || (old.is_none() && !l.is_none()) {
            l.space = if l.is_none() { 0 } else { space.max(0) };
        }
    }
    let shading = st.shading;
    let shading_color = st.shading_color;
    let borders_changed = b != st.base.0;
    let shading_changed = (shading, shading_color) != (st.base.1, st.base.2);
    if d.selected("applyto") > 0 && d.is_enabled("applyto") {
        if borders_changed || shading_changed {
            apply_to_cells(
                b,
                st.base.0,
                (shading, shading_color),
                borders_changed,
                shading_changed,
                d.selected("applyto"),
                view,
            );
        }
        return true;
    }
    if borders_changed || shading_changed {
        view.format_paras("Border", move |p| {
            if borders_changed {
                p.borders = b;
            }
            if shading_changed {
                p.shading = shading;
                p.shading_color = shading_color;
            }
        });
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::Document;

    fn dialog_state() -> (Dialog, BorderState) {
        let view = DocView::new(Document::new(), "Test".to_string());
        border(&view)
    }

    #[test]
    fn a_line_with_no_side_picked_makes_a_box() {
        let (mut d, mut st) = dialog_state();
        d.set_selected("lines", 4);
        changed(&mut st, &mut d, "lines");
        assert_eq!(st.line, BorderStyle::Thick);
        assert_eq!(st.borders.top.style, BorderStyle::Thick);
        assert_eq!(st.borders.left.style, BorderStyle::Thick);
        assert!(st.borders.between.is_none());
        assert!(d.is_on("top"));
    }

    #[test]
    fn turning_off_a_side_keeps_the_other_sides() {
        let (mut d, mut st) = dialog_state();
        changed(&mut st, &mut d, "p_box");
        d.set_on("left", false);
        changed(&mut st, &mut d, "left");
        assert!(st.borders.left.is_none());
        assert_eq!(st.borders.top.style, BorderStyle::Single);
        assert_eq!(st.sel, TOP | BOTTOM | RIGHT);
    }

    #[test]
    fn side_checks_combine() {
        let (mut d, mut st) = dialog_state();
        d.set_on("top", true);
        changed(&mut st, &mut d, "top");
        d.set_on("bottom", true);
        changed(&mut st, &mut d, "bottom");
        assert_eq!(st.sel, TOP | BOTTOM);
    }

    #[test]
    fn border_uses_only_standard_controls() {
        let (d, _) = dialog_state();
        assert!(
            !d.items
                .iter()
                .any(|item| matches!(item.kind, crate::dialog::Kind::Custom))
        );
    }

    #[test]
    fn unchanged_dialog_preserves_exact_fill_and_unequal_border_distances() {
        let mut doc = Document::new();
        let p = &mut doc.main.paras[0].pap;
        p.shading_color = Some(Rgb(0xEBF0FA));
        p.borders.top = BorderLine {
            style: BorderStyle::Single,
            space: 241,
            ..Default::default()
        };
        p.borders.left = BorderLine {
            style: BorderStyle::Single,
            space: 122,
            ..Default::default()
        };
        let original = p.clone();
        let mut view = DocView::new(doc, "Test".into());
        let (d, st) = border(&view);
        assert!(apply(&st, &d, &mut view));
        assert_eq!(view.doc.main.paras[0].pap, original);
        assert_eq!(view.doc.undo_label(), None);
    }

    #[test]
    fn border_only_edits_preserve_each_paragraphs_fill_and_undo_redo() {
        use quill_core::{Position, Selection};
        let mut doc = quill_core::text::from_plain_text("One\nTwo");
        let colors = [Some(Rgb(0xEBF0FA)), Some(Rgb(0x123456))];
        for (p, color) in doc.main.paras.iter_mut().zip(colors) {
            p.pap.shading_color = color;
        }
        let mut view = DocView::new(doc, "Test".into());
        view.set_selection(Selection::new(Position::new(0, 0), Position::new(1, 3)));
        let (mut d, mut st) = border(&view);
        changed(&mut st, &mut d, "p_box");
        assert!(apply(&st, &d, &mut view));
        assert_eq!(
            view.doc
                .main
                .paras
                .iter()
                .map(|p| p.pap.shading_color)
                .collect::<Vec<_>>(),
            colors
        );
        assert!(!view.doc.main.paras[0].pap.borders.is_none());
        view.doc.undo();
        assert!(view.doc.main.paras[0].pap.borders.is_none());
        view.doc.redo();
        assert_eq!(view.doc.main.paras[1].pap.shading_color, colors[1]);
    }

    #[test]
    fn cell_target_displays_its_fill_and_border_edits_keep_other_cell_colors() {
        use quill_core::{Position, Selection, StoryId};
        let mut doc = Document::new();
        doc.insert_table(StoryId::Main, Position::new(0, 0), 1, 2, 3000);
        let row = doc
            .main
            .paras
            .iter_mut()
            .find_map(|p| p.row.as_mut())
            .unwrap();
        row.cells[0].shading_color = Some(Rgb(0xEBF0FA));
        row.cells[1].shading_color = Some(Rgb(0xABCDEF));
        let mut view = DocView::new(doc, "Test".into());
        view.set_selection(Selection::new(Position::new(0, 0), Position::new(1, 0)));
        let (mut d, st) = border(&view);
        d.set_selected("applyto", 1);
        let mut st = for_cells(st, &view, &mut d);
        changed(&mut st, &mut d, "p_box");
        assert!(apply(&st, &d, &mut view));
        let row = view
            .doc
            .main
            .paras
            .iter()
            .find_map(|p| p.row.as_ref())
            .unwrap();
        assert_eq!(row.cells[0].shading_color, Some(Rgb(0xEBF0FA)));
        assert_eq!(row.cells[1].shading_color, Some(Rgb(0xABCDEF)));
    }

    #[test]
    fn a_line_color_goes_to_the_sides_picked() {
        let mut view = DocView::new(Document::new(), "Test".to_string());
        let (mut d, mut st) = border(&view);
        assert_eq!(d.swatch_rgb("line_color"), Some(0));
        changed(&mut st, &mut d, "p_box");
        set_color(&mut st, &mut d, "line_color", Some(Rgb(0xFF0000)));
        assert_eq!(st.borders.top.color, Rgb(0xFF0000));
        assert_eq!(d.item_label("line_color_hex"), "#FF0000");
        assert!(apply(&st, &d, &mut view));
        assert_eq!(view.doc.main.paras[0].pap.borders.left.color, Rgb(0xFF0000));
    }

    #[test]
    fn a_background_color_is_plain_and_clears_to_none() {
        let mut view = DocView::new(Document::new(), "Test".to_string());
        view.doc.main.paras[0].pap.shading = 200;
        let (mut d, mut st) = border(&view);
        set_color(&mut st, &mut d, "shading_color", Some(Rgb(0x3366CC)));
        assert_eq!(d.swatch_rgb("shading_color"), Some(0x3366CC));
        assert!(apply(&st, &d, &mut view));
        let p = &view.doc.main.paras[0].pap;
        assert_eq!((p.shading, p.shading_color), (0, Some(Rgb(0x3366CC))));
        let (mut d, mut st) = border(&view);
        set_color(&mut st, &mut d, "shading_color", None);
        assert!(apply(&st, &d, &mut view));
        assert_eq!(view.doc.main.paras[0].pap.shading_color, None);
    }

    #[test]
    fn choices_survive_a_change_of_target_and_a_table_target_reaches_every_cell() {
        use quill_core::{Position, Selection, StoryId};
        let mut doc = Document::new();
        doc.insert_table(StoryId::Main, Position::new(0, 0), 2, 2, 3000);
        let mut view = DocView::new(doc, "Test".into());
        view.set_selection(Selection::caret(Position::new(0, 0)));
        let (mut d, mut st) = border(&view);
        changed(&mut st, &mut d, "p_box");
        set_color(&mut st, &mut d, "line_color", Some(Rgb(0x3366CC)));
        d.set_selected("applyto", 1);
        target_changed(&mut st, &mut d, &view);
        assert_eq!(st.color, Rgb(0x3366CC));
        assert_eq!(d.swatch_rgb("line_color"), Some(0x3366CC));
        d.set_selected("applyto", 2);
        target_changed(&mut st, &mut d, &view);
        assert!(apply(&st, &d, &mut view));
        let lines: Vec<_> = view
            .doc
            .main
            .paras
            .iter()
            .filter_map(|p| p.row.as_ref())
            .flat_map(|r| r.cells.iter().map(|c| c.borders.top.color))
            .collect();
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().all(|c| *c == Rgb(0x3366CC)));
    }

    #[test]
    fn a_changed_side_is_also_set_on_the_cell_across_it() {
        use quill_core::{Position, Selection, StoryId};
        let mut doc = Document::new();
        doc.insert_table(StoryId::Main, Position::new(0, 0), 2, 2, 3000);
        let mut view = DocView::new(doc, "Test".into());
        view.set_selection(Selection::caret(Position::new(0, 0)));
        let (mut d, mut st) = border_at(&view, 1);
        changed(&mut st, &mut d, "p_box");
        set_color(&mut st, &mut d, "line_color", Some(Rgb(0xFF0000)));
        assert!(apply(&st, &d, &mut view));
        let rows: Vec<_> = view
            .doc
            .main
            .paras
            .iter()
            .filter_map(|p| p.row.as_ref())
            .collect();
        // The top left cell's right edge is the top right cell's left edge,
        // and its bottom edge is the bottom left cell's top edge.
        assert_eq!(rows[0].cells[1].borders.left.color, Rgb(0xFF0000));
        assert_eq!(rows[1].cells[0].borders.top.color, Rgb(0xFF0000));
        // Cells that do not touch it are left alone.
        assert!(rows[1].cells[1].borders.is_none());
    }

    #[test]
    fn a_selected_whole_table_opens_on_table_and_applies_to_every_cell() {
        use quill_core::{Position, Selection, StoryId};
        let mut doc = Document::new();
        doc.insert_table(StoryId::Main, Position::new(0, 0), 2, 2, 3000);
        let mut view = DocView::new(doc, "Test".into());
        view.set_selection(Selection::caret(Position::new(0, 0)));
        view.select_table();
        let (mut d, mut st) = border_at(&view, 2);
        changed(&mut st, &mut d, "p_box");
        assert!(apply(&st, &d, &mut view));
        assert!(
            view.doc
                .main
                .paras
                .iter()
                .filter_map(|p| p.row.as_ref())
                .all(|r| r.cells.iter().all(|c| !c.borders.is_none()))
        );
    }
}
