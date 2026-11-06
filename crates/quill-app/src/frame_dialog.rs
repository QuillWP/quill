//! Format > Frame and Insert > Drop Cap, laid out from the original.

use crate::dialog::{Dialog, units};
use crate::docview::DocView;
use quill_core::props::{Frame, FrameH, FrameRel, FrameV};

const H_NAMES: [&str; 5] = ["Left", "Center", "Right", "Inside", "Outside"];
const V_NAMES: [&str; 4] = ["In line", "Top", "Center", "Bottom"];
const H_RELS: [&str; 3] = ["Margin", "Page", "Column"];
const V_RELS: [&str; 2] = ["Margin", "Page"];
/// The original's default distance from the text, 0.13 in.
const FROM_TEXT: i32 = 180;

fn h_text(h: FrameH) -> String {
    match h {
        FrameH::Left => "Left".into(),
        FrameH::Center => "Center".into(),
        FrameH::Right => "Right".into(),
        FrameH::Inside => "Inside".into(),
        FrameH::Outside => "Outside".into(),
        FrameH::At(t) => units::length(t),
    }
}

fn v_text(v: FrameV) -> String {
    match v {
        FrameV::InLine => "In line".into(),
        FrameV::Top => "Top".into(),
        FrameV::Center => "Center".into(),
        FrameV::Bottom => "Bottom".into(),
        FrameV::At(t) => units::length(t),
    }
}

/// Format > Frame.
pub fn frame(view: &DocView) -> Dialog {
    let pap = view.current_pap();
    let f = pap.frame.unwrap_or_default();
    let mut d = Dialog::new("Frame", 352.0, 154.0);
    d.group("Horizontal", 5.0, 5.0, 257.0, 46.0);
    d.field("h", &h_text(f.h), 11.0, 21.0, 65.0);
    d.menu_button("h_menu", "", &H_NAMES, 79.0, 21.0, 19.0);
    d.text("relative to", 105.0, 23.0);
    let h_rel = match f.h_rel {
        FrameRel::Margin => 0,
        FrameRel::Page => 1,
        FrameRel::Column => 2,
    };
    d.popup("h_rel", &H_RELS, h_rel, 182.0, 21.0, 76.0);
    d.group("Vertical", 5.0, 51.0, 257.0, 47.0);
    d.field("v", &v_text(f.v), 11.0, 67.0, 65.0);
    d.menu_button("v_menu", "", &V_NAMES, 79.0, 67.0, 19.0);
    d.text("relative to", 105.0, 69.0);
    let v_rel = usize::from(f.v_rel == FrameRel::Page);
    d.popup("v_rel", &V_RELS, v_rel, 182.0, 67.0, 76.0);
    d.text("Frame Width:", 8.0, 104.0);
    let width = f.width.map_or("Auto".to_string(), units::length);
    d.field("width", &width, 101.0, 101.0, 65.0);
    d.menu_button("width_menu", "", &["Auto"], 169.0, 101.0, 19.0);
    d.text_as("from_label", "From Text:", 8.0, 130.0);
    d.field("from", &units::length(f.from_text), 101.0, 127.0, 66.0);
    d.button("ok", "OK", 272.0, 15.0, 71.0, true);
    d.button("cancel", "Cancel", 272.0, 40.0, 71.0, false);
    d.button("unframe", "Unframe", 272.0, 64.0, 71.0, false);
    d.button("position", "Position…", 272.0, 101.0, 71.0, false);
    d.enable("unframe", pap.frame.is_some());
    changed(&mut d, "");
    d.focus("h");
    d
}

/// Keep the combo fields and the dimmed controls in step.
pub fn changed(d: &mut Dialog, id: &str) {
    match id {
        "h_menu" => {
            let k = d.selected("h_menu");
            d.set_field("h", H_NAMES.get(k).copied().unwrap_or("Left"));
        }
        "v_menu" => {
            let k = d.selected("v_menu");
            d.set_field("v", V_NAMES.get(k).copied().unwrap_or("In line"));
            // Moving a frame off its line gives it room from the text.
            if k > 0 && units::parse_length(&d.field_text("from")) == Some(0) {
                d.set_field("from", &units::length(FROM_TEXT));
            }
        }
        "width_menu" => d.set_field("width", "Auto"),
        _ => {}
    }
    let inline = d.field_text("v").trim().eq_ignore_ascii_case("in line");
    d.enable("v_rel", !inline);
}

/// The frame the dialog describes, or None if a field is not valid.
pub fn read(d: &Dialog) -> Option<Frame> {
    let name_or = |text: String, names: &[&str]| -> Option<Result<usize, i32>> {
        let t = text.trim().to_string();
        if let Some(k) = names.iter().position(|n| n.eq_ignore_ascii_case(&t)) {
            return Some(Ok(k));
        }
        units::parse_length(&t).map(Err)
    };
    let h = match name_or(d.field_text("h"), &H_NAMES)? {
        Ok(0) => FrameH::Left,
        Ok(1) => FrameH::Center,
        Ok(2) => FrameH::Right,
        Ok(3) => FrameH::Inside,
        Ok(_) => FrameH::Outside,
        Err(t) => FrameH::At(t),
    };
    let v = match name_or(d.field_text("v"), &V_NAMES)? {
        Ok(0) => FrameV::InLine,
        Ok(1) => FrameV::Top,
        Ok(2) => FrameV::Center,
        Ok(_) => FrameV::Bottom,
        Err(t) => FrameV::At(t),
    };
    let width = match d.field_text("width").trim() {
        w if w.eq_ignore_ascii_case("auto") || w.is_empty() => None,
        w => Some(units::parse_length(w)?.max(144)),
    };
    let from_text = units::parse_length(&d.field_text("from"))?.max(0);
    Some(Frame {
        h,
        h_rel: match d.selected("h_rel") {
            0 => FrameRel::Margin,
            1 => FrameRel::Page,
            _ => FrameRel::Column,
        },
        v,
        v_rel: if d.selected("v_rel") == 1 {
            FrameRel::Page
        } else {
            FrameRel::Margin
        },
        width,
        from_text,
        drop_lines: 0,
        in_margin: false,
    })
}

// ----- Drop Cap -----

pub fn drop_cap(view: &DocView, fonts: &[String]) -> Dialog {
    let mut d = Dialog::new("Drop Cap", 336.0, 185.0);
    let chp = view.current_chp();
    let current = view.doc.font_name(chp.font).to_string();
    let mut names = fonts.to_vec();
    if !names.iter().any(|f| f.eq_ignore_ascii_case(&current)) {
        names.insert(0, current.clone());
    }
    let sel = names
        .iter()
        .position(|f| f.eq_ignore_ascii_case(&current))
        .unwrap_or(0);
    d.text("Font:", 10.0, 5.0);
    d.popup_owned("font", names, sel, 9.0, 30.0, 151.0);
    d.text("Height:", 172.0, 5.0);
    d.field("height", "3 li", 172.0, 30.0, 51.0);
    let heights: Vec<String> = (1..=10).map(|n| format!("{n} li")).collect();
    let refs: Vec<&str> = heights.iter().map(String::as_str).collect();
    d.menu_button("height_menu", "", &refs, 227.0, 30.0, 19.0);
    d.button("ok", "OK", 259.0, 16.0, 62.0, true);
    d.button("cancel", "Cancel", 258.0, 45.0, 64.0, false);
    d.check("smallcaps", "Small Caps", 11.0, 61.0, false);
    d.group("Position", 9.0, 82.0, 154.0, 95.0);
    d.radio("dropped", "Dropped", 15.0, 104.0, 1, true);
    d.radio("margin", "In Margin", 15.0, 126.0, 1, false);
    // Keep these as ordinary text controls so the complete dialog can be
    // hosted by AppKit. They are deliberately small, typographic previews:
    // the first lets following text wrap below the capital, while the second
    // keeps the capital outside the text margin.
    d.text_as("icon_dropped", "W  text", 111.0, 103.0);
    d.text_as("icon_margin", "W | text", 111.0, 125.0);
    d.group("Apply To", 170.0, 82.0, 154.0, 95.0);
    d.radio("char", "First Character", 175.0, 104.0, 2, true);
    d.radio("word", "First Word", 175.0, 126.0, 2, false);
    d.radio("sel", "Whole Selection", 175.0, 148.0, 2, false);
    d.enable(
        "sel",
        !view.sel.is_empty() && view.sel.start().para == view.sel.end().para,
    );
    drop_cap_changed(&mut d, "");
    d.focus("height");
    d
}

pub fn drop_cap_changed(d: &mut Dialog, id: &str) {
    if id == "height_menu" {
        let k = d.selected("height_menu");
        d.set_field("height", &format!("{} li", k + 1));
    }
    // Small capitals only make sense for more than one letter.
    let one = d.radio_on(2) == "char";
    d.enable("smallcaps", !one);
    if one {
        d.set_on("smallcaps", false);
    }
}

/// What the Drop Cap dialog asks for.
pub struct DropCap {
    pub font: String,
    pub lines: u8,
    pub in_margin: bool,
    pub small_caps: bool,
    /// 0 first character, 1 first word, 2 the selection.
    pub apply_to: u8,
}

pub fn read_drop_cap(d: &Dialog) -> Option<DropCap> {
    let h = d.field_text("height");
    let lines: u8 = h
        .trim()
        .trim_end_matches("li")
        .trim()
        .parse()
        .ok()
        .filter(|n| (1..=10).contains(n))?;
    Some(DropCap {
        font: d.selected_text("font"),
        lines,
        in_margin: d.radio_on(1) == "margin",
        small_caps: d.is_on("smallcaps"),
        apply_to: match d.radio_on(2) {
            "word" => 1,
            "sel" => 2,
            _ => 0,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialog::Kind;
    use quill_core::Document;

    #[test]
    fn drop_cap_uses_only_standard_controls() {
        let view = DocView::new(Document::new(), "Test".to_string());
        let dialog = drop_cap(&view, &["Helvetica".to_string()]);

        assert!(
            !dialog
                .items
                .iter()
                .any(|item| matches!(item.kind, Kind::Custom))
        );
        assert!(dialog.items.iter().any(|item| item.id == "icon_dropped"));
        assert!(dialog.items.iter().any(|item| item.id == "icon_margin"));
    }
}
