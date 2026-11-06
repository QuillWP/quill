//! Format > Picture and the right-click Properties...: the picture's size,
//! its scale and how much is cropped from each edge.

use crate::dialog::{Dialog, units};
use quill_core::{Picture, Position};

/// Fields for the four edges, in the order of `Picture::crop_on_page`.
const EDGES: [&str; 4] = ["left", "top", "right", "bottom"];

/// What the dialog is working on. The sizes are kept as exact numbers, since
/// the fields show them rounded.
#[derive(Clone, Debug)]
pub struct PictureState {
    pub at: Position,
    pub picture: Picture,
    /// The size before cropping, as the dialog opened and as it is now.
    first: (f32, f32),
    size: (f32, f32),
    /// What each edge's field means, when it hasn't been typed in.
    crop: [f32; 4],
}

fn percent(size: f32, natural: i32) -> String {
    format!("{:.0}", 100.0 * size / natural.max(1) as f32)
}

fn twips(t: f32) -> i32 {
    t.round() as i32
}

pub fn dialog(at: Position, picture: Picture) -> (Dialog, PictureState) {
    let size = picture.full_size();
    let crop = picture.crop_on_page();
    let (nw, nh) = picture.natural;
    let mut d = Dialog::new("Picture", 348.0, 202.0);
    d.group("Size", 5.0, 5.0, 258.0, 95.0);
    d.text("Width:", 11.0, 24.0);
    d.field("width", &units::length(twips(size.0)), 62.0, 21.0, 65.0);
    d.text("Height:", 136.0, 24.0);
    d.field("height", &units::length(twips(size.1)), 188.0, 21.0, 65.0);
    d.text("Scale:", 11.0, 50.0);
    d.field("scale_w", &percent(size.0, nw), 62.0, 47.0, 45.0);
    d.text("%", 110.0, 50.0);
    d.text("Scale:", 136.0, 50.0);
    d.field("scale_h", &percent(size.1, nh), 188.0, 47.0, 45.0);
    d.text("%", 236.0, 50.0);
    let (a, b) = (
        100.0 * size.0 / nw.max(1) as f32,
        100.0 * size.1 / nh.max(1) as f32,
    );
    d.check("lock", "Keep Proportions", 11.0, 76.0, (a - b).abs() < 0.5);
    d.group("Crop", 5.0, 104.0, 258.0, 67.0);
    d.text("Left:", 11.0, 123.0);
    d.field("left", &units::length(twips(crop[0])), 62.0, 120.0, 65.0);
    d.text("Right:", 136.0, 123.0);
    d.field("right", &units::length(twips(crop[2])), 188.0, 120.0, 65.0);
    d.text("Top:", 11.0, 149.0);
    d.field("top", &units::length(twips(crop[1])), 62.0, 146.0, 65.0);
    d.text("Bottom:", 136.0, 149.0);
    d.field("bottom", &units::length(twips(crop[3])), 188.0, 146.0, 65.0);
    d.text_as("final", "", 11.0, 180.0);
    d.button("ok", "OK", 272.0, 15.0, 71.0, true);
    d.button("cancel", "Cancel", 272.0, 40.0, 71.0, false);
    let st = PictureState {
        at,
        picture,
        first: size,
        size,
        crop,
    };
    show_final(&mut d, &st);
    d.focus("width");
    (d, st)
}

/// What the field for edge `i` says, in twips.
fn edge(d: &Dialog, st: &PictureState, i: usize) -> Option<f32> {
    let text = d.field_text(EDGES[i]);
    if text == units::length(twips(st.crop[i])) {
        return Some(st.crop[i]);
    }
    let t = units::parse_length_from(&text, twips(st.crop[i]))?;
    (t >= 0).then_some(t as f32)
}

/// The size after cropping, if the fields describe one.
fn final_size(d: &Dialog, st: &PictureState) -> Option<(f32, f32)> {
    let e = |i| edge(d, st, i);
    let (w, h) = st.size;
    let shown = (w - e(0)? - e(2)?, h - e(1)? - e(3)?);
    (shown.0 >= 20.0 && shown.1 >= 20.0).then_some(shown)
}

fn show_final(d: &mut Dialog, st: &PictureState) {
    let label = match final_size(d, st) {
        Some((w, h)) => format!(
            "Final size: {} \u{00D7} {}",
            units::length(twips(w)),
            units::length(twips(h))
        ),
        None => "Final size: nothing would show".to_string(),
    };
    d.set_label("final", &label);
}

/// Keep the fields in step as they are typed in. With Keep Proportions on,
/// a change to any size or scale changes the others by the same amount.
/// Crops are measured on the page, so they scale with the picture.
pub fn changed(d: &mut Dialog, st: &mut PictureState, id: &str) {
    let (nw, nh) = st.picture.natural;
    let (nw, nh) = (nw.max(1) as f32, nh.max(1) as f32);
    let number = |d: &Dialog, id: &str| {
        let text = d.field_text(id);
        text.trim().trim_end_matches('%').trim().parse::<f32>().ok()
    };
    let (w, h) = st.size;
    let new_width = match id {
        "width" => units::parse_length(&d.field_text(id)).map(|t| t as f32),
        "scale_w" => number(d, id).map(|p| nw * p / 100.0),
        _ => None,
    };
    let new_height = match id {
        "height" => units::parse_length(&d.field_text(id)).map(|t| t as f32),
        "scale_h" => number(d, id).map(|p| nh * p / 100.0),
        _ => None,
    };
    let lock = d.is_on("lock");
    let (nw2, nh2) = match (new_width, new_height) {
        (Some(new), _) if new >= 20.0 => (new, if lock { h * new / w } else { h }),
        (_, Some(new)) if new >= 20.0 => (if lock { w * new / h } else { w }, new),
        _ => (w, h),
    };
    if (nw2, nh2) != (w, h) {
        let (kx, ky) = (nw2 / w, nh2 / h);
        let shown = [st.crop[0], st.crop[1], st.crop[2], st.crop[3]];
        let typed: Vec<Option<f32>> = (0..4).map(|i| edge(d, st, i)).collect();
        st.size = (nw2, nh2);
        for i in 0..4 {
            let k = if i % 2 == 0 { kx } else { ky };
            st.crop[i] = shown[i] * k;
            if let Some(t) = typed[i]
                && t > 0.0
            {
                let t = if t == shown[i] { st.crop[i] } else { t * k };
                d.set_field(EDGES[i], &units::length(twips(t)));
            }
        }
        if id != "width" {
            d.set_field("width", &units::length(twips(nw2)));
        }
        if id != "height" {
            d.set_field("height", &units::length(twips(nh2)));
        }
        if id != "scale_w" {
            d.set_field("scale_w", &format!("{:.0}", 100.0 * nw2 / nw));
        }
        if id != "scale_h" {
            d.set_field("scale_h", &format!("{:.0}", 100.0 * nh2 / nh));
        }
    }
    show_final(d, st);
}

/// The picture the dialog describes, or None if a field is not valid. Only
/// what changed is touched, so OK on its own changes nothing.
pub fn read(d: &Dialog, st: &PictureState) -> Option<Picture> {
    final_size(d, st)?;
    let mut new = st.picture.clone();
    if st.size != st.first {
        let (kx, ky) = (st.size.0 / st.first.0, st.size.1 / st.first.1);
        new = new.scaled_to(
            (new.size.0 as f32 * kx).round() as i32,
            (new.size.1 as f32 * ky).round() as i32,
        );
    }
    let now = new.crop_on_page();
    let mut more = [0.0; 4];
    for i in 0..4 {
        let value = edge(d, st, i)?;
        if value != st.crop[i] {
            more[i] = value - now[i];
        }
    }
    Some(if more == [0.0; 4] {
        new
    } else {
        new.crop_edges(more)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture 7 x 5 in at 100%.
    fn picture() -> Picture {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&(7u32 * 72).to_be_bytes());
        png.extend_from_slice(&(5u32 * 72).to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0]);
        Picture::from_bytes(png).unwrap()
    }

    fn opened() -> (Dialog, PictureState) {
        crate::settings::update(|s| s.unit = crate::settings::Unit::Inch);
        dialog(Position::new(0, 0), picture())
    }

    #[test]
    fn ok_on_its_own_changes_nothing() {
        let (d, st) = opened();
        assert!(read(&d, &st).unwrap() == st.picture);
    }

    #[test]
    fn the_final_size_follows_the_crop_as_it_is_typed() {
        let (mut d, mut st) = opened();
        for id in EDGES {
            d.set_field(id, "0.5 in");
            changed(&mut d, &mut st, id);
        }
        assert_eq!(d.item_label("final"), "Final size: 6 in \u{00D7} 4 in");
        let new = read(&d, &st).unwrap();
        assert_eq!(new.size, (6 * 1440, 4 * 1440));
        assert!((new.full_size().0 - 7.0 * 1440.0).abs() < 2.0);
        // Too much to leave anything showing: the dialog stays open.
        d.set_field("left", "8 in");
        changed(&mut d, &mut st, "left");
        assert!(read(&d, &st).is_none());
    }

    #[test]
    fn the_lock_keeps_proportions_and_unlocked_each_side_is_free() {
        let (mut d, mut st) = opened();
        assert!(d.is_on("lock"));
        d.set_field("scale_w", "50");
        changed(&mut d, &mut st, "scale_w");
        assert_eq!(d.field_text("scale_h"), "50");
        assert_eq!(d.field_text("width"), "3.5 in");
        assert_eq!(d.field_text("height"), "2.5 in");
        d.set_on("lock", false);
        d.set_field("height", "5 in");
        changed(&mut d, &mut st, "height");
        assert_eq!(d.field_text("scale_h"), "100");
        assert_eq!(d.field_text("scale_w"), "50");
        let new = read(&d, &st).unwrap();
        assert_eq!(new.size, (5040, 5 * 1440));
    }

    #[test]
    fn half_typed_numbers_leave_the_others_alone() {
        let (mut d, mut st) = opened();
        d.set_field("width", "0");
        changed(&mut d, &mut st, "width");
        assert_eq!(d.field_text("height"), "5 in");
        d.set_field("width", "");
        changed(&mut d, &mut st, "width");
        assert_eq!(d.field_text("height"), "5 in");
    }
}
