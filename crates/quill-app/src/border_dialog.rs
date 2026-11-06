//! Format > Border: a preview of two paragraphs where you pick the sides,
//! a list of line styles, presets, distance from text and shading. Layout
//! measured from the original.

use crate::dialog::{Dialog, units};
use crate::docview::DocView;
use quill_core::props::{BorderLine, BorderStyle, Borders, Rgb, SHADING_STEPS};

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
    /// Sides picked in the preview.
    pub sel: u8,
    /// The line style chosen in the list.
    pub line: BorderStyle,
    /// The line color chosen.
    pub color: Rgb,
}

/// The line colors offered: the standard ones and what the line has now,
/// without the entry that opens the palette.
fn color_choices(have: Rgb) -> (Vec<String>, usize) {
    let (mut names, at) = crate::format_dialogs::color_options(Some(have), false);
    names.pop();
    (names, at)
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

fn shading_label(v: u16) -> String {
    match v {
        0 => "None".into(),
        1000 => "Solid".into(),
        v => {
            let s = format!("{:.1}", v as f32 / 10.0);
            format!("{}%", s.trim_end_matches(".0"))
        }
    }
}

fn shading_choices(shading: u16, color: Option<Rgb>) -> (Vec<String>, usize) {
    let mut names: Vec<String> = SHADING_STEPS.iter().map(|&v| shading_label(v)).collect();
    let (colors, at) = color_choices(color.unwrap_or(Rgb::BLACK));
    let selected = color.map_or_else(
        || {
            SHADING_STEPS
                .iter()
                .position(|&v| v == shading)
                .unwrap_or(0)
        },
        |_| names.len() + at,
    );
    names.extend(colors);
    (names, selected)
}

/// Refresh controls after choosing a different target for the formatting.
fn sync_controls(st: &BorderState, d: &mut Dialog) {
    let (names, selected) = shading_choices(st.shading, st.shading_color);
    d.set_options("shading", names, selected);
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
    let (names, at) = color_choices(st.color);
    d.set_options("line_color", names, at);
    d.set_selected(
        "lines",
        LINES
            .iter()
            .position(|(_, style)| *style == st.line)
            .unwrap_or(3),
    );
    update_preview(st, d);
}

pub fn target_changed(st: &mut BorderState, d: &mut Dialog, view: &DocView) {
    if d.selected("applyto") == 1 {
        *st = for_cells(st.clone(), view, d);
    } else {
        let (_, fresh) = border(view);
        *st = fresh;
        sync_controls(st, d);
    }
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
    d.check("top", "Top", 18.0, 22.0, sel & TOP != 0);
    d.check("bottom", "Bottom", 18.0, 40.0, sel & BOTTOM != 0);
    d.check("left", "Left", 18.0, 58.0, sel & LEFT != 0);
    d.check("right", "Right", 78.0, 58.0, sel & RIGHT != 0);
    d.check("between", "Between", 18.0, 76.0, sel & BETWEEN != 0);
    d.note("preview0", "", 19.0, 98.0, 135.0);
    d.note("preview1", "", 19.0, 112.0, 135.0);
    d.note("preview2", "", 19.0, 126.0, 135.0);
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
    let (colors, color_at) = color_choices(color);
    d.popup_owned("line_color", colors, color_at, 174.0, 97.0, 104.0);
    d.button("ok", "OK", 304.0, 15.0, 52.0, true);
    d.button("cancel", "Cancel", 304.0, 45.0, 52.0, false);
    d.text("Shading:", 169.0, 160.0);
    let (shades, at) = shading_choices(p.shading, p.shading_color);
    d.popup_owned("shading", shades, at, 169.0, 177.0, 113.0);
    d.text_as("swatch", "", 290.0, 180.0);
    d.text_as("applyto_label", "Apply To:", 169.0, 203.0);
    d.popup("applyto", &["Paragraph", "Cells"], 0, 169.0, 217.0, 186.0);
    // Apply To: Cells only means something in a table.
    if !p.in_table {
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

/// Start from the selected cells' borders and shading, for Apply To:
/// Cells (Table Cells' Border… button).
pub fn for_cells(mut st: BorderState, view: &DocView, d: &mut Dialog) -> BorderState {
    if let Some(ts) = crate::table_dialogs::table_selection(view) {
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
    sync_controls(&st, d);
    st
}

/// Put borders and shading on the selected cells.
fn apply_to_cells(
    b: Borders,
    shading: u16,
    shading_color: Option<Rgb>,
    borders_changed: bool,
    shading_changed: bool,
    view: &mut DocView,
) {
    let Some(ts) = crate::table_dialogs::table_selection(view) else {
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
    let sel = view.sel;
    view.doc.commit(sel);
}

/// Keep the standard controls and the live native preview in step.
pub fn changed(st: &mut BorderState, d: &mut Dialog, id: &str) {
    match id {
        "shading" => {
            if let Some(&shade) = SHADING_STEPS.get(d.selected("shading")) {
                st.shading = shade;
                st.shading_color = None;
            } else if let Some(color) =
                crate::format_dialogs::color_named(&d.selected_text("shading"))
            {
                st.shading = 0;
                st.shading_color = Some(color);
            }
        }
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
        "line_color" => {
            st.color = crate::format_dialogs::color_named(&d.selected_text("line_color"))
                .unwrap_or(Rgb::BLACK);
            for bit in [TOP, BOTTOM, LEFT, RIGHT, BETWEEN] {
                if st.sel & bit != 0 {
                    side(&mut st.borders, bit).color = st.color;
                }
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
    let left = if st.borders.left.is_none() {
        " "
    } else {
        "│"
    };
    let right = if st.borders.right.is_none() {
        " "
    } else {
        "│"
    };
    let top = if st.borders.top.is_none() {
        format!("{left}  Paragraph  {right}")
    } else {
        "┌──────────────┐".to_string()
    };
    let between = if st.borders.between.is_none() {
        format!("{left}  Paragraph  {right}")
    } else {
        "├──────────────┤".to_string()
    };
    let bottom = if st.borders.bottom.is_none() {
        format!("{left}  Paragraph  {right}")
    } else {
        "└──────────────┘".to_string()
    };
    d.set_label("preview0", &top);
    d.set_label("preview1", &between);
    d.set_label("preview2", &bottom);
    let line_name = LINES
        .iter()
        .find(|(_, style)| *style == st.line)
        .map_or("None", |(name, _)| *name);
    d.set_label("line_preview", &format!("{line_name} border"));
    d.set_label(
        "swatch",
        &st.shading_color
            .map_or_else(|| shading_label(st.shading), |c| format!("#{:06X}", c.0)),
    );
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
    if d.selected("applyto") == 1 && d.is_enabled("applyto") {
        if borders_changed || shading_changed {
            apply_to_cells(
                b,
                shading,
                shading_color,
                borders_changed,
                shading_changed,
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
        assert_eq!(d.selected_text("shading"), "#EBF0FA");
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
        d.set_selected("shading", 0);
        changed(&mut st, &mut d, "shading");
        assert!(apply(&st, &d, &mut view));
        assert!(
            view.doc
                .main
                .paras
                .iter()
                .all(|p| p.pap.shading_color.is_none())
        );
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
        assert_eq!(d.selected_text("shading"), "#EBF0FA");
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
    fn shading_labels() {
        assert_eq!(shading_label(0), "None");
        assert_eq!(shading_label(125), "12.5%");
        assert_eq!(shading_label(200), "20%");
        assert_eq!(shading_label(1000), "Solid");
    }
}
