//! The color palette: a row of standard colors and a row of custom ones
//! that Quill remembers (the last used, newest first), as in a drawing
//! program. Double-clicking a color opens an editor with a slider and a
//! number for each of red, green and blue, and an HTML color code, so the
//! exact color can be set (and written down).

use crate::canvas::{Canvas, Rect};
use crate::dialog::{Dialog, PopItem, Popout};
use quill_core::Document;
use quill_core::props::Rgb;
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// The standard colors, in the order they are shown: grays, then the
/// bright colors (the original's eight are all here), then three dark ones.
pub const STANDARD: [Rgb; 16] = [
    Rgb(0x000000),
    Rgb(0x404040),
    Rgb(0x808080),
    Rgb(0xC0C0C0),
    Rgb(0xFFFFFF),
    Rgb(0xFF0000),
    Rgb(0xFF8000),
    Rgb(0xFFFF00),
    Rgb(0x00FF00),
    Rgb(0x00FFFF),
    Rgb(0x0000FF),
    Rgb(0x8000FF),
    Rgb(0xFF00FF),
    Rgb(0x800000),
    Rgb(0x008000),
    Rgb(0x000080),
];

/// How many custom colors are kept.
pub const SLOTS: usize = 16;

/// A box's size and the distance between the starts of two, in points.
const BOX: f32 = 20.0;
const PITCH: f32 = 22.0;

const DOUBLE_CLICK: Duration = Duration::from_millis(450);

/// "#RRGGBB".
pub fn hex(c: Rgb) -> String {
    format!("#{:06X}", c.0 & 0xFF_FFFF)
}

/// A color as an HTML color code: "#RRGGBB" or "RRGGBB", or the short
/// "#RGB".
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let t = text.trim().trim_start_matches('#');
    if !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match t.len() {
        6 => u32::from_str_radix(t, 16).ok().map(Rgb),
        3 => {
            let v = u32::from_str_radix(t, 16).ok()?;
            let (r, g, b) = ((v >> 8) & 0xF, (v >> 4) & 0xF, v & 0xF);
            Some(Rgb(((r * 17) << 16) | ((g * 17) << 8) | (b * 17)))
        }
        _ => None,
    }
}

/// One box of the palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Standard(usize),
    Custom(usize),
}

#[derive(Clone, Debug)]
pub struct PaletteState {
    /// The color chosen now.
    pub current: Rgb,
    /// What the color was when the palette opened, for Cancel.
    pub start: Option<Rgb>,
    pub selected: Option<Slot>,
    /// The custom colors, newest first.
    pub custom: Vec<Rgb>,
    last_click: Option<(Instant, Slot)>,
}

thread_local! {
    /// Colors edited from the Character dialog's pop-out. They wait for
    /// the dialog's OK, so its Cancel leaves the custom colors as they were.
    static PENDING: RefCell<Vec<(Target, Rgb)>> = const { RefCell::new(Vec::new()) };
}

impl PaletteState {
    /// The palette on `start` (black if none), with the saved colors, the
    /// colors `doc` uses, and any edits waiting for their dialog's OK.
    pub fn new(start: Option<Rgb>, doc: &Document) -> PaletteState {
        let mut custom = custom_colors(doc);
        PENDING.with_borrow(|pending| {
            for &(target, new) in pending {
                place_edit(&mut custom, target, new);
            }
        });
        let current = start.unwrap_or(Rgb::BLACK);
        let mut st = PaletteState {
            current,
            start,
            selected: None,
            custom,
            last_click: None,
        };
        st.selected = st.slot_of(current);
        st
    }

    fn slot_of(&self, c: Rgb) -> Option<Slot> {
        STANDARD
            .iter()
            .position(|s| *s == c)
            .map(Slot::Standard)
            .or_else(|| self.custom.iter().position(|s| *s == c).map(Slot::Custom))
    }

    fn color_at(&self, slot: Slot) -> Option<Rgb> {
        match slot {
            Slot::Standard(i) => STANDARD.get(i).copied(),
            Slot::Custom(i) => self.custom.get(i).copied(),
        }
    }
}

/// What a click in the palette asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    None,
    /// Edit this box (double-clicked).
    Edit(Slot),
}

/// The palette, opened on `start` (or black), and its state.
pub fn palette(start: Option<Rgb>, doc: &Document) -> (Dialog, PaletteState) {
    let st = PaletteState::new(start, doc);
    let current = st.current;
    let row = PITCH * SLOTS as f32 - (PITCH - BOX);
    let mut d = Dialog::new("Color", row + 28.0, 222.0);
    d.text("Standard colors:", 14.0, 12.0);
    d.custom("standard", 14.0, 30.0, row, BOX);
    d.text("Custom colors (double-click one to edit it):", 14.0, 62.0);
    d.custom("custom", 14.0, 80.0, row, BOX);
    d.swatch("preview", 14.0, 118.0, 44.0, 44.0, current.0);
    d.text("HTML:", 72.0, 124.0);
    d.field("hex", &hex(current), 112.0, 120.0, 84.0);
    d.button("edit", "Edit…", 210.0, 118.0, 80.0, false);
    d.button("ok", "OK", row - 150.0, 182.0, 74.0, true);
    d.button(
        "cancel",
        "Cancel",
        row - 70.0 + 28.0 - 14.0,
        182.0,
        74.0,
        false,
    );
    (d, st)
}

/// Draw the two rows of boxes.
pub fn draw(st: &PaletteState, id: &str, r: Rect, c: &mut Canvas) {
    let standard = id == "standard";
    for i in 0..SLOTS {
        let slot = if standard {
            Slot::Standard(i)
        } else {
            Slot::Custom(i)
        };
        let b = Rect::new(r.x + i as f32 * PITCH, r.y, BOX, BOX);
        match st.color_at(slot) {
            Some(rgb) => {
                c.fill(b, rgb.0);
                c.frame(b, 0x808080);
            }
            None => c.frame(b, 0xC8C8C8),
        }
        if st.selected == Some(slot) {
            c.frame(b.inset(-2.0), 0x0A84FF);
            c.frame(b.inset(-3.0), 0x0A84FF);
        }
    }
}

/// Show the color chosen: the preview box and the HTML field.
fn sync(st: &PaletteState, d: &mut Dialog) {
    d.set_swatch("preview", st.current.0);
    d.set_field("hex", &hex(st.current));
}

/// A click at (`x`, `y`) in the "standard" or "custom" row.
pub fn clicked(st: &mut PaletteState, d: &mut Dialog, id: &str, x: f32, y: f32) -> Click {
    let i = (x / PITCH).floor();
    if x < 0.0 || y < 0.0 || y > BOX || i < 0.0 || x - i * PITCH > BOX || i as usize >= SLOTS {
        return Click::None;
    }
    let i = i as usize;
    let slot = if id == "standard" {
        Slot::Standard(i)
    } else {
        Slot::Custom(i)
    };
    st.selected = Some(slot);
    if let Some(c) = st.color_at(slot) {
        st.current = c;
        sync(st, d);
    }
    let now = Instant::now();
    let double = st
        .last_click
        .is_some_and(|(t, s)| s == slot && now - t < DOUBLE_CLICK);
    st.last_click = Some((now, slot));
    if double {
        st.last_click = None;
        return Click::Edit(slot);
    }
    Click::None
}

/// The HTML field was typed in.
pub fn changed(st: &mut PaletteState, d: &mut Dialog, id: &str) {
    if id != "hex" {
        return;
    }
    if let Some(c) = parse_hex(&d.field_text("hex")) {
        st.current = c;
        st.selected = st.slot_of(c);
        d.set_swatch("preview", c.0);
    }
}

/// Where an edited color goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Over this custom color.
    Replace(usize),
    /// Added to the custom colors.
    Add,
    /// In place of this color, deleted from the document.
    Instead(Rgb),
}

/// What the Edit button, or a double-click on `slot`, starts from, and
/// where the result goes.
pub fn edit_of(st: &PaletteState, slot: Option<Slot>) -> (Rgb, Target) {
    match slot.or(st.selected) {
        Some(Slot::Custom(i)) if i < st.custom.len() => (st.custom[i], Target::Replace(i)),
        Some(Slot::Standard(i)) => (STANDARD[i], Target::Add),
        _ => (st.current, Target::Add),
    }
}

/// An edited color came back from the editor.
pub fn edited(st: &mut PaletteState, d: &mut Dialog, target: Target, new: Rgb) {
    match target {
        Target::Replace(i) if i < st.custom.len() => {
            st.custom[i] = new;
            st.selected = Some(Slot::Custom(i));
        }
        _ => {
            st.custom.retain(|c| *c != new);
            st.custom.insert(0, new);
            st.custom.truncate(SLOTS);
            st.selected = Some(Slot::Custom(0));
        }
    }
    st.current = new;
    sync(st, d);
}

/// OK: the color chosen, added to the row while the palette is open.
pub fn finish(st: &mut PaletteState) -> Rgb {
    let c = st.current;
    if !STANDARD.contains(&c) && !st.custom.contains(&c) {
        st.custom.insert(0, c);
        st.custom.truncate(SLOTS);
    }
    c
}

// ----- The pop-out -----

const STD_IDS: [&str; SLOTS] = [
    "std0", "std1", "std2", "std3", "std4", "std5", "std6", "std7", "std8", "std9", "std10",
    "std11", "std12", "std13", "std14", "std15",
];
const CUS_IDS: [&str; SLOTS] = [
    "cus0", "cus1", "cus2", "cus3", "cus4", "cus5", "cus6", "cus7", "cus8", "cus9", "cus10",
    "cus11", "cus12", "cus13", "cus14", "cus15",
];

/// The pop-out beside a color control: the standard colors, the custom
/// ones under them, and an Edit… button, the color now chosen marked.
/// `delete` adds a Delete button for the document's own colors.
pub fn popout(st: &PaletteState, anchor: &'static str, delete: bool) -> Popout {
    const PAD: f32 = 8.0;
    let row = PITCH * SLOTS as f32 - (PITCH - BOX);
    let mut items = Vec::new();
    for (k, y) in [(0, PAD), (1, PAD + BOX + 8.0)] {
        for i in 0..SLOTS {
            let (id, slot) = if k == 0 {
                (STD_IDS[i], Slot::Standard(i))
            } else {
                (CUS_IDS[i], Slot::Custom(i))
            };
            items.push(PopItem {
                id,
                rect: crate::canvas::Rect::new(PAD + i as f32 * PITCH, y, BOX, BOX),
                rgb: st.color_at(slot).map(|c| c.0),
                selected: st.selected == Some(slot),
                label: "",
                tip: st.color_at(slot).map(tip),
                enabled: true,
            });
        }
    }
    let y = PAD + 2.0 * BOX + 8.0 + 8.0;
    items.push(PopItem {
        id: "edit",
        rect: crate::canvas::Rect::new(PAD, y, 84.0, 22.0),
        rgb: None,
        selected: false,
        label: "Edit Color…",
        tip: None,
        enabled: true,
    });
    if delete {
        items.push(PopItem {
            id: "delete_color",
            rect: crate::canvas::Rect::new(PAD + 92.0, y, 64.0, 22.0),
            rgb: None,
            selected: false,
            label: "Delete",
            tip: Some("Delete a color this document uses".to_string()),
            enabled: deletable(st).is_some(),
        });
    }
    Popout {
        anchor,
        size: (row + 2.0 * PAD, y + 22.0 + PAD),
        items,
    }
}

/// The box a pop-out id names.
pub fn slot_of_id(id: &str) -> Option<Slot> {
    STD_IDS
        .iter()
        .position(|s| *s == id)
        .map(Slot::Standard)
        .or_else(|| CUS_IDS.iter().position(|s| *s == id).map(Slot::Custom))
}

/// What pressing a box of the pop-out asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// The color of that box was chosen.
    Chose(Rgb),
    /// The box was double-clicked: edit it.
    Edit(Slot),
}

/// A box of the pop-out was pressed. None for an empty custom box.
pub fn pop_swatch(st: &mut PaletteState, id: &str, double: bool) -> Option<Pick> {
    let slot = slot_of_id(id)?;
    let color = st.color_at(slot)?;
    st.selected = Some(slot);
    st.current = color;
    Some(if double {
        Pick::Edit(slot)
    } else {
        Pick::Chose(color)
    })
}

/// The custom row: the colors saved in Preferences for every document,
/// then the colors this document uses. A color made for one document
/// stays with it, in its text.
fn custom_colors(doc: &Document) -> Vec<Rgb> {
    let mut out: Vec<Rgb> = crate::settings::with(|s| s.named_colors.clone())
        .into_iter()
        .map(|(c, _)| Rgb(c))
        .collect();
    let stories = std::iter::once(&doc.main)
        .chain(doc.headers.iter())
        .chain([&doc.notes, &doc.annotations]);
    for p in stories.flat_map(|s| &s.paras) {
        for run in p.runs.iter().filter(|r| r.len > 0) {
            let c = run.chp.color;
            if !STANDARD.contains(&c) && !out.contains(&c) {
                out.push(c);
            }
        }
        if out.len() >= SLOTS {
            break;
        }
    }
    out.truncate(SLOTS);
    out
}

/// What pointing at a box shows: the color's name, or its HTML code.
pub fn tip(c: Rgb) -> String {
    name_of(c)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| hex(c))
}

/// The color Delete would remove: a custom one chosen in the pop-out that
/// is the document's own, not one saved in Preferences for every document.
pub fn deletable(st: &PaletteState) -> Option<Rgb> {
    let Some(Slot::Custom(i)) = st.selected else {
        return None;
    };
    let c = *st.custom.get(i)?;
    let saved = crate::settings::with(|s| s.named_colors.iter().any(|(rgb, _)| *rgb == c.0));
    (!saved).then_some(c)
}

/// Whether any text in `doc` is in color `c`.
pub fn uses(doc: &Document, c: Rgb) -> bool {
    std::iter::once(&doc.main)
        .chain(doc.headers.iter())
        .chain([&doc.notes, &doc.annotations])
        .flat_map(|s| &s.paras)
        .any(|p| p.runs.iter().any(|r| r.len > 0 && r.chp.color == c))
}

/// Take color `c` out of the custom row, and out of the edits waiting
/// for their dialog's OK.
pub fn forget(st: &mut PaletteState, c: Rgb) {
    st.custom.retain(|x| *x != c);
    drop_edits_of(c);
    if st.current == c {
        st.current = Rgb::BLACK;
    }
    st.selected = st.slot_of(st.current);
}

/// The name saved for a color in Preferences, if it has one.
pub fn name_of(c: Rgb) -> Option<String> {
    crate::settings::with(|s| {
        s.named_colors
            .iter()
            .find(|(rgb, _)| *rgb == c.0)
            .map(|(_, name)| name.clone())
    })
}

/// A color edited from the pop-out, held until its dialog's OK.
pub fn hold_edit(target: Target, new: Rgb) {
    PENDING.with_borrow_mut(|pending| pending.push((target, new)));
}

/// The dialog is done: forget the colors edited from it. One that was
/// applied is in the document's text now, and shows from there.
pub fn drop_edits() {
    PENDING.with_borrow_mut(Vec::clear);
}

/// Forget the waiting edits that made color `c`.
pub fn drop_edits_of(c: Rgb) {
    PENDING.with_borrow_mut(|pending| pending.retain(|(_, x)| *x != c));
}

/// Put an edited color where the edit said.
fn place_edit(custom: &mut Vec<Rgb>, target: Target, new: Rgb) {
    match target {
        Target::Replace(i) if i < custom.len() => custom[i] = new,
        _ => {
            custom.retain(|c| *c != new);
            custom.insert(0, new);
            custom.truncate(SLOTS);
        }
    }
}

// ----- The editor -----

#[derive(Clone, Debug)]
pub struct EditState {
    pub target: Target,
}

const CHANNELS: [(&str, &str, &str, u32); 3] = [
    ("Red:", "r", "r_slider", 16),
    ("Green:", "g", "g_slider", 8),
    ("Blue:", "b", "b_slider", 0),
];

fn parts(c: Rgb) -> [i32; 3] {
    [c.r() as i32, c.g() as i32, c.b() as i32]
}

fn join(v: [i32; 3]) -> Rgb {
    Rgb((v[0].clamp(0, 255) as u32) << 16
        | (v[1].clamp(0, 255) as u32) << 8
        | v[2].clamp(0, 255) as u32)
}

/// The color editor, started on `start`.
pub fn editor(start: Rgb, target: Target) -> (Dialog, EditState) {
    let mut d = Dialog::new("Edit Color", 340.0, 214.0);
    let v = parts(start);
    for (k, (label, field, slider, _)) in CHANNELS.into_iter().enumerate() {
        let y = 16.0 + 30.0 * k as f32;
        d.text(label, 14.0, y + 2.0);
        d.slider(slider, 62.0, y, 200.0, 255, v[k], (0, 0));
        d.field(field, &v[k].to_string(), 274.0, y - 3.0, 52.0);
    }
    d.text("HTML:", 14.0, 112.0);
    d.field("hex", &hex(start), 62.0, 108.0, 90.0);
    d.swatch("before", 176.0, 104.0, 70.0, 36.0, start.0);
    d.swatch("after", 246.0, 104.0, 70.0, 36.0, start.0);
    d.text("Before", 176.0, 146.0);
    d.text("After", 246.0, 146.0);
    d.button("ok", "OK", 176.0, 176.0, 70.0, true);
    d.button("cancel", "Cancel", 256.0, 176.0, 70.0, false);
    d.focus("hex");
    let st = EditState { target };
    refresh(&mut d, start, None);
    (d, st)
}

/// The color the editor shows now.
pub fn edit_color(d: &Dialog) -> Rgb {
    join([
        d.slider_value("r_slider"),
        d.slider_value("g_slider"),
        d.slider_value("b_slider"),
    ])
}

/// Bring the sliders' tracks, the numbers, the HTML code and the "After"
/// box in line with `c`. The field named by `skip` is the one being typed
/// in, and is left alone.
fn refresh(d: &mut Dialog, c: Rgb, skip: Option<&str>) {
    let v = parts(c);
    for (k, (_, field, slider, shift)) in CHANNELS.into_iter().enumerate() {
        let (mut lo, mut hi) = (v, v);
        lo[k] = 0;
        hi[k] = 255;
        let _ = shift;
        d.set_slider(slider, v[k], (join(lo).0, join(hi).0));
        if skip != Some(field) {
            d.set_field(field, &v[k].to_string());
        }
    }
    if skip != Some("hex") {
        d.set_field("hex", &hex(c));
    }
    d.set_swatch("after", c.0);
}

/// A slider moved or a field was typed in.
pub fn edit_changed(_: &mut EditState, d: &mut Dialog, id: &str) {
    match id {
        "r_slider" | "g_slider" | "b_slider" => {
            let c = edit_color(d);
            refresh(d, c, None);
        }
        "r" | "g" | "b" => {
            let Ok(n) = d.field_text(id).trim().parse::<i32>() else {
                return;
            };
            let mut v = parts(edit_color(d));
            let k = CHANNELS.iter().position(|c| c.1 == id).unwrap_or(0);
            v[k] = n.clamp(0, 255);
            refresh(d, join(v), Some(id));
        }
        "hex" => {
            if let Some(c) = parse_hex(&d.field_text("hex")) {
                refresh(d, c, Some("hex"));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_codes_read_with_or_without_the_hash_and_short() {
        assert_eq!(parse_hex("#FF8000"), Some(Rgb(0xFF8000)));
        assert_eq!(parse_hex("ff8000"), Some(Rgb(0xFF8000)));
        assert_eq!(parse_hex(" #f80 "), Some(Rgb(0xFF8800)));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#GG0000"), None);
        assert_eq!(hex(Rgb(0x0A0B0C)), "#0A0B0C");
    }

    #[test]
    fn the_editor_keeps_sliders_numbers_and_code_together() {
        let (mut d, mut st) = editor(Rgb(0x102030), Target::Add);
        assert_eq!(d.field_text("r"), "16");
        assert_eq!(d.field_text("hex"), "#102030");
        // A slider moved.
        d.set_slider("g_slider", 200, (0, 0));
        edit_changed(&mut st, &mut d, "g_slider");
        assert_eq!(d.field_text("g"), "200");
        assert_eq!(d.field_text("hex"), "#10C830");
        assert_eq!(edit_color(&d), Rgb(0x10C830));
        // A number typed.
        d.set_field("b", "255");
        edit_changed(&mut st, &mut d, "b");
        assert_eq!(d.slider_value("b_slider"), 255);
        assert_eq!(d.field_text("hex"), "#10C8FF");
        // An HTML code typed: the others follow, and it stays as typed.
        d.set_field("hex", "#abc");
        edit_changed(&mut st, &mut d, "hex");
        assert_eq!(edit_color(&d), Rgb(0xAABBCC));
        assert_eq!(d.field_text("hex"), "#abc");
        assert_eq!(d.field_text("r"), "170");
        // Nonsense changes nothing.
        d.set_field("r", "x");
        edit_changed(&mut st, &mut d, "r");
        assert_eq!(edit_color(&d), Rgb(0xAABBCC));
    }

    #[test]
    fn choosing_editing_and_remembering_colors() {
        let (mut d, mut st) = palette(Some(Rgb(0xFF0000)), &Document::new());
        st.custom.clear();
        // Red is a standard color, selected to begin with.
        assert_eq!(st.selected, Some(Slot::Standard(5)));
        // A click picks a color; a second on the same box edits it.
        assert_eq!(
            clicked(&mut st, &mut d, "standard", 2.0 * PITCH + 3.0, 5.0),
            Click::None
        );
        assert_eq!(st.current, Rgb(0x808080));
        assert_eq!(d.field_text("hex"), "#808080");
        let slot = match clicked(&mut st, &mut d, "standard", 2.0 * PITCH + 3.0, 5.0) {
            Click::Edit(s) => s,
            other => panic!("{other:?}"),
        };
        let (start, target) = edit_of(&st, Some(slot));
        assert_eq!((start, target), (Rgb(0x808080), Target::Add));
        // The edited color goes to the front of the custom row.
        edited(&mut st, &mut d, target, Rgb(0x123456));
        assert_eq!(st.custom, vec![Rgb(0x123456)]);
        assert_eq!(st.selected, Some(Slot::Custom(0)));
        assert_eq!(d.field_text("hex"), "#123456");
        // Editing a custom box changes it in place.
        let (start, target) = edit_of(&st, Some(Slot::Custom(0)));
        assert_eq!((start, target), (Rgb(0x123456), Target::Replace(0)));
        edited(&mut st, &mut d, target, Rgb(0x654321));
        assert_eq!(st.custom, vec![Rgb(0x654321)]);
        // The HTML code picks a color too, and OK remembers one that is
        // new once, newest first, and never a standard one.
        d.set_field("hex", "#ABCDEF");
        changed(&mut st, &mut d, "hex");
        assert_eq!(finish(&mut st), Rgb(0xABCDEF));
        assert_eq!(st.custom, vec![Rgb(0xABCDEF), Rgb(0x654321)]);
        st.current = Rgb(0xFF0000);
        finish(&mut st);
        assert_eq!(st.custom, vec![Rgb(0xABCDEF), Rgb(0x654321)]);
        // Sixteen at most.
        for n in 0..20 {
            st.current = Rgb(0x010101 * (n + 1));
            finish(&mut st);
        }
        assert_eq!(st.custom.len(), SLOTS);
    }

    #[test]
    fn an_edited_color_shows_until_its_dialog_is_done() {
        let doc = Document::new();
        let dropped = Rgb(0x0A0B0C);
        hold_edit(Target::Add, dropped);
        assert!(PaletteState::new(None, &doc).custom.contains(&dropped));
        drop_edits();
        assert!(!PaletteState::new(None, &doc).custom.contains(&dropped));
    }

    #[test]
    fn custom_colors_are_the_saved_ones_and_the_documents_own() {
        let mut doc = Document::new();
        let own = Rgb(0x123123);
        let chp = quill_core::CharProps {
            color: own,
            ..doc.styles.get(0).chp
        };
        doc.main.paras[0].insert(0, "Hello", chp);
        crate::settings::update(|s| s.named_colors = vec![(0xFF0000 + 7, "Company".into())]);
        let row = custom_colors(&doc);
        crate::settings::update(|s| s.named_colors.clear());
        assert_eq!(row, [Rgb(0xFF0007), own]);
        assert!(custom_colors(&Document::new()).is_empty());
    }
}
