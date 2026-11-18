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

/// The palette uses two airy rows of eight colors instead of one long strip.
const BOX: f32 = 24.0;
const GAP: f32 = 6.0;
const PITCH: f32 = BOX + GAP;
const COLS: usize = 8;
const GRID_W: f32 = COLS as f32 * BOX + (COLS - 1) as f32 * GAP;
const GRID_H: f32 = BOX + PITCH;

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
        6 => Rgb::from_hex(t),
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
    let mut d = Dialog::new("Color", 340.0, 300.0);
    d.text("Standard colors:", 14.0, 12.0);
    d.custom("standard", 14.0, 30.0, GRID_W, GRID_H);
    d.text("Recent and document colors:", 14.0, 96.0);
    d.custom("custom", 14.0, 114.0, GRID_W, GRID_H);
    d.swatch("preview", 14.0, 184.0, 44.0, 44.0, current.0);
    d.text("HTML:", 72.0, 190.0);
    d.field("hex", &hex(current), 112.0, 186.0, 84.0);
    d.button("edit", "More Colors…", 210.0, 184.0, 116.0, false);
    d.button("delete_color", "Delete", 210.0, 216.0, 116.0, false);
    d.enable("delete_color", deletable(&st).is_some());
    d.action_buttons(&[("cancel", "Cancel", 74.0, false), ("ok", "OK", 74.0, true)]);
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
        let b = Rect::new(
            r.x + (i % COLS) as f32 * PITCH,
            r.y + (i / COLS) as f32 * PITCH,
            BOX,
            BOX,
        );
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
    d.enable("delete_color", deletable(st).is_some());
}

/// A click at (`x`, `y`) in the "standard" or "custom" row.
pub fn clicked(st: &mut PaletteState, d: &mut Dialog, id: &str, x: f32, y: f32) -> Click {
    let col = (x / PITCH).floor();
    let row = (y / PITCH).floor();
    if x < 0.0
        || y < 0.0
        || col < 0.0
        || row < 0.0
        || x - col * PITCH > BOX
        || y - row * PITCH > BOX
        || col as usize >= COLS
        || row as usize >= SLOTS / COLS
    {
        return Click::None;
    }
    let i = row as usize * COLS + col as usize;
    let slot = if id == "standard" {
        Slot::Standard(i)
    } else {
        Slot::Custom(i)
    };
    st.selected = Some(slot);
    if let Some(c) = st.color_at(slot) {
        st.current = c;
    }
    sync(st, d);
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
        d.enable("delete_color", deletable(st).is_some());
    }
}

/// Where an edited color goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Over this custom color, which was `Rgb` and changes wherever it is used.
    Replace(usize, Rgb),
    /// Added to the custom colors.
    Add,
    /// In place of this color, deleted from the document. The name is the
    /// color box the palette came out of.
    Instead(Rgb, &'static str),
    /// Applied to the selected text, from the toolbar's palette.
    Text,
}

/// What the Edit button, or a double-click on `slot`, starts from, and
/// where the result goes.
pub fn edit_of(st: &PaletteState, slot: Option<Slot>) -> (Rgb, Target) {
    match slot.or(st.selected) {
        Some(Slot::Custom(i)) if i < st.custom.len() => {
            (st.custom[i], Target::Replace(i, st.custom[i]))
        }
        Some(Slot::Standard(i)) => (STANDARD[i], Target::Add),
        _ => (st.current, Target::Add),
    }
}

/// An edited color came back from the editor.
pub fn edited(st: &mut PaletteState, d: &mut Dialog, target: Target, new: Rgb) {
    match target {
        Target::Replace(i, _) if i < st.custom.len() => {
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
/// ones under them, and a More Colors… button, the color now chosen marked.
/// A Delete button takes the custom color chosen out of the palette. Every
/// color palette in Quill is this one pop-out, so they all behave alike.
pub fn popout(st: &PaletteState, anchor: &'static str) -> Popout {
    const PAD: f32 = 12.0;
    let mut items = Vec::new();
    for (id, label, y) in [
        ("standard_label", "Standard colors", PAD),
        ("custom_label", "Recent and document colors", PAD + 84.0),
    ] {
        items.push(PopItem {
            id,
            rect: crate::canvas::Rect::new(PAD, y, GRID_W, 18.0),
            rgb: None,
            selected: false,
            label,
            framed: false,
            tip: None,
            enabled: false,
        });
    }
    for (k, y) in [(0, PAD + 22.0), (1, PAD + 106.0)] {
        for i in 0..SLOTS {
            let (id, slot) = if k == 0 {
                (STD_IDS[i], Slot::Standard(i))
            } else {
                (CUS_IDS[i], Slot::Custom(i))
            };
            items.push(PopItem {
                id,
                rect: crate::canvas::Rect::new(
                    PAD + (i % COLS) as f32 * PITCH,
                    y + (i / COLS) as f32 * PITCH,
                    BOX,
                    BOX,
                ),
                rgb: st.color_at(slot).map(|c| c.0),
                selected: st.selected == Some(slot),
                label: "",
                framed: false,
                tip: st.color_at(slot).map(tip),
                enabled: true,
            });
        }
    }
    let y = PAD + 174.0;
    items.push(PopItem {
        id: "edit",
        rect: crate::canvas::Rect::new(PAD, y, 116.0, 24.0),
        rgb: None,
        selected: false,
        label: "More Colors…",
        framed: true,
        tip: None,
        enabled: true,
    });
    items.push(PopItem {
        id: "delete_color",
        rect: crate::canvas::Rect::new(PAD + 124.0, y, 72.0, 24.0),
        rgb: None,
        selected: false,
        label: "Delete",
        framed: true,
        tip: Some("Delete the custom color chosen".to_string()),
        enabled: deletable(st).is_some(),
    });
    Popout {
        anchor,
        size: (GRID_W + 2.0 * PAD, y + 24.0 + PAD),
        items,
    }
}

/// What a press on item `id` of a pop-out means to the dialog it is in: a
/// box, or the Edit or Delete button. None for anything else. The Mac, Windows
/// and Linux front ends all turn their controls into outcomes with this, so
/// a button added to the pop-out works on every one of them.
pub fn pop_outcome(id: &'static str, double: bool) -> Option<crate::dialog::Outcome> {
    use crate::dialog::Outcome;
    match id {
        "edit" => Some(Outcome::Button("edit")),
        "delete_color" => Some(Outcome::Button("delete_color")),
        _ if slot_of_id(id).is_some() => Some(Outcome::Swatch { id, double }),
        _ => None,
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

/// The color Delete would remove: the custom one chosen. The standard
/// colors stay.
pub fn deletable(st: &PaletteState) -> Option<Rgb> {
    let Some(Slot::Custom(i)) = st.selected else {
        return None;
    };
    st.custom.get(i).copied()
}

/// Whether Preferences saves `c` as one of its colors for every document.
pub fn is_saved(c: Rgb) -> bool {
    crate::settings::with(|s| s.named_colors.iter().any(|(rgb, _)| *rgb == c.0))
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
///
/// The row is built again from what Preferences saves and what `doc` uses,
/// so a color that was both stays in the row as the document's own after
/// Preferences lets go of it.
pub fn forget(st: &mut PaletteState, c: Rgb, doc: &Document) {
    drop_edits_of(c);
    let mut custom = custom_colors(doc);
    PENDING.with_borrow(|pending| {
        for &(target, new) in pending {
            place_edit(&mut custom, target, new);
        }
    });
    st.custom = custom;
    if st.slot_of(st.current).is_none() {
        st.current = Rgb::BLACK;
    }
    st.selected = st.slot_of(st.current);
}

/// The standalone Color dialog after color `old` is gone: its row is built
/// again and the dialog shows the color chosen now.
pub fn forgotten(st: &mut PaletteState, d: &mut Dialog, old: Rgb, doc: &Document) {
    forget(st, old, doc);
    sync(st, d);
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
        Target::Replace(i, _) if i < custom.len() => custom[i] = new,
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
    d.action_buttons(&[("cancel", "Cancel", 70.0, false), ("ok", "OK", 70.0, true)]);
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
        assert_eq!(
            (start, target),
            (Rgb(0x123456), Target::Replace(0, Rgb(0x123456)))
        );
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
