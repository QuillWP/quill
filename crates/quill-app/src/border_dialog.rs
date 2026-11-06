//! Format > Border: a preview of two paragraphs where you pick the sides,
//! a list of line styles, presets, distance from text and shading. Layout
//! measured from the original.

use crate::dialog::{Dialog, units};
use crate::docview::DocView;
use quill_core::props::{BorderLine, BorderStyle, Borders, SHADING_STEPS};

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
    base: (Borders, u16),
    pub borders: Borders,
    pub shading: u16,
    /// Sides picked in the preview.
    pub sel: u8,
    /// The line style chosen in the list.
    pub line: BorderStyle,
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
    d.button("ok", "OK", 304.0, 15.0, 52.0, true);
    d.button("cancel", "Cancel", 304.0, 45.0, 52.0, false);
    d.text("Shading:", 169.0, 160.0);
    let shades: Vec<String> = SHADING_STEPS.iter().map(|&v| shading_label(v)).collect();
    let at = SHADING_STEPS
        .iter()
        .position(|&v| v == p.shading)
        .unwrap_or(0);
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
        base: (b, p.shading),
        borders: b,
        shading: p.shading,
        sel,
        line,
    };
    update_preview(&st, &mut d);
    (d, st)
}

/// Start from the selected cells' borders and shading, for Apply To:
/// Cells (Table Cells' Border… button).
pub fn for_cells(mut st: BorderState, view: &DocView) -> BorderState {
    if let Some(ts) = crate::table_dialogs::table_selection(view) {
        let paras = &view.doc.main.paras;
        let p = quill_core::table::row_props(paras, &ts.rows[0], view.doc.props.text_width());
        let cell = &p.cells[ts.from.min(p.cells.len() - 1)];
        st.borders = cell.borders;
        st.shading = cell.shading;
        st.base = (cell.borders, cell.shading);
        st.sel = 0;
    }
    st
}

/// Put borders and shading on the selected cells.
fn apply_to_cells(b: Borders, shading: u16, view: &mut DocView) {
    let Some(ts) = crate::table_dialogs::table_selection(view) else {
        return;
    };
    let tw = view.doc.props.text_width();
    view.doc.begin("Border", view.sel);
    for row in &ts.rows {
        let mut p = quill_core::table::row_props(&view.doc.main.paras, row, tw);
        let n = p.cells.len();
        for k in ts.from.min(n - 1)..=ts.to.min(n - 1) {
            p.cells[k].borders = b;
            p.cells[k].shading = shading;
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
            st.shading = SHADING_STEPS[d.selected("shading").min(SHADING_STEPS.len() - 1)];
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
                    side(&mut st.borders, bit).style = st.line;
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
            side(&mut st.borders, bit).style = st.line;
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
    d.set_label("swatch", &shading_label(st.shading));
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
    let Some(space) = units::parse(&d.field_text("space"), "pt") else {
        return false;
    };
    let mut b = st.borders;
    for bit in [TOP, BOTTOM, LEFT, RIGHT, BETWEEN] {
        let l = side(&mut b, bit);
        l.space = if l.is_none() { 0 } else { space.max(0) };
    }
    let shading = st.shading;
    if d.selected("applyto") == 1 && d.is_enabled("applyto") {
        if (b, shading) != st.base {
            apply_to_cells(b, shading, view);
        }
        return true;
    }
    if (b, shading) != st.base {
        view.format_paras("Border", move |p| {
            p.borders = b;
            p.shading = shading;
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
    fn shading_labels() {
        assert_eq!(shading_label(0), "None");
        assert_eq!(shading_label(125), "12.5%");
        assert_eq!(shading_label(200), "20%");
        assert_eq!(shading_label(1000), "Solid");
    }
}
