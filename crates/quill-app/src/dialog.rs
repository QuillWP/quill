//! Platform-neutral dialog definitions and flat fallback controls. A dialog knows how to
//! draw itself and handle input; what the values mean is up to whoever
//! built it.

use crate::canvas::{Canvas, Label, Rect, color, round_rect};
use crate::menu::{self, Item as MenuItem, Menu, OpenMenu};
use quill_layout::{FontFamilyInfo, FontVariant};

const SIZE: f32 = 12.0;
const MODERN_FONT: &str = quill_layout::fonts::UI_FONT;

fn modern_box(c: &mut Canvas, r: Rect, radius: f32, border: u32, fill: u32) {
    round_rect(c, r, radius, border);
    let hair = c.hair();
    let inner = r.inset(hair);
    round_rect(c, inner, (radius - hair).max(1.0), fill);
}

#[derive(Clone, Debug)]
pub enum Kind {
    Button {
        default: bool,
    },
    Check {
        on: bool,
        /// Three-state checkbox (Find's formats): gray means "either".
        tri: bool,
        mixed: bool,
    },
    Radio {
        group: u8,
        on: bool,
    },
    Field {
        text: String,
        sel: (usize, usize),
    },
    Popup {
        options: Vec<String>,
        selected: usize,
        font_families: Option<Vec<FontFamilyInfo>>,
    },
    /// Static text.
    Text,
    /// Small explanatory interface text.
    Note,
    /// A titled frame around other items.
    Group,
    /// An area the dialog's owner draws and handles clicks in (a preview,
    /// a list of samples, an icon).
    Custom,
    /// A box of one color (0xRRGGBB), with a thin frame. As a `button`
    /// it can be pressed, like a color well.
    Swatch {
        rgb: u32,
        button: bool,
    },
    /// A slider from 0 to `max`, its track shading from the color `from`
    /// to `to`. Pressing or dragging it sets `value` and reports a change.
    Slider {
        value: i32,
        max: i32,
        from: u32,
        to: u32,
    },
    /// A scrolling list of rows, one chosen. A double-click on a row presses
    /// button `double`, when there is one.
    List {
        rows: Vec<String>,
        state: crate::listbox::ListBox,
        double: Option<&'static str>,
    },
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: &'static str,
    pub label: String,
    pub rect: Rect,
    pub kind: Kind,
    pub enabled: bool,
    /// A static label can remain visible while showing that it is overridden.
    pub struck: bool,
}

impl Item {
    fn new(id: &'static str, label: &str, rect: Rect, kind: Kind) -> Item {
        Item {
            id,
            label: label.to_string(),
            rect,
            kind,
            enabled: true,
            struck: false,
        }
    }

    fn takes_focus(&self) -> bool {
        self.enabled && matches!(self.kind, Kind::Field { .. })
    }
}

/// What a dialog event means to its owner.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    None,
    /// A value changed (checkbox, radio, popup, field).
    Changed(&'static str),
    /// A button other than OK and Cancel was pressed.
    Button(&'static str),
    /// A custom item was clicked at this point, relative to its top left;
    /// the flag is whether Shift was down.
    Clicked(&'static str, f32, f32, bool),
    /// A box of a pop-out was pressed; `double` is whether it was the
    /// second press of a double-click.
    Swatch {
        id: &'static str,
        double: bool,
    },
    /// OK (or Return), Cancel (or Escape, ⌘.), or Apply.
    Ok,
    Cancel,
    Apply,
}

/// A small panel that pops out beside one of a dialog's items: boxes of
/// color, and perhaps a button. Its places are in points from its own top
/// left; it opens below its `anchor` item.
#[derive(Clone, Debug, PartialEq)]
pub struct Popout {
    pub anchor: &'static str,
    pub size: (f32, f32),
    pub items: Vec<PopItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PopItem {
    pub id: &'static str,
    pub rect: Rect,
    /// The color of a box, or None for an empty one (or a button).
    pub rgb: Option<u32>,
    pub selected: bool,
    /// A button's text; empty for a box.
    pub label: &'static str,
    /// What the pointer shows over it.
    pub tip: Option<String>,
    /// False for a button that does nothing now.
    pub enabled: bool,
}

/// A popped-out panel at `panel`: a soft shadow, the frame, then its boxes
/// and buttons. Dialogs and the toolbar's color palette both use it.
pub fn draw_popout_panel(c: &mut Canvas, label: &mut Label, panel: Rect, p: &Popout) {
    // A soft shadow, then the panel.
    c.fill(
        Rect::new(panel.x + 1.0, panel.y + 2.0, panel.w, panel.h),
        0xC4C4C4,
    );
    modern_box(c, panel, 6.0, 0xA8A8A8, 0xFFFFFF);
    for it in &p.items {
        let r = Rect::new(
            panel.x + it.rect.x,
            panel.y + it.rect.y,
            it.rect.w,
            it.rect.h,
        );
        if !it.label.is_empty() {
            modern_box(c, r, 5.0, 0xB5B5B5, 0xF4F4F4);
            let ink = if it.enabled { 0x202020 } else { 0xA0A0A0 };
            let w = label.width(MODERN_FONT, SIZE, false, it.label);
            label.draw(
                c,
                MODERN_FONT,
                SIZE,
                false,
                r.x + (r.w - w) / 2.0,
                r.y + r.h / 2.0 + 4.5,
                it.label,
                ink,
            );
            continue;
        }
        match it.rgb {
            Some(rgb) => {
                c.fill(r, rgb);
                c.frame(r, 0x808080);
            }
            None => c.frame(r, 0xC8C8C8),
        }
        if it.selected {
            c.frame(r.inset(-2.0), 0x0A84FF);
            c.frame(r.inset(-3.0), 0x0A84FF);
        }
    }
}

pub struct Dialog {
    pub title: String,
    /// A platform window title when the classic dialog deliberately has no
    /// title bar of its own (for example Go To).
    pub native_title: Option<String>,
    pub width: f32,
    pub height: f32,
    pub items: Vec<Item>,
    /// Top-left on screen, set when the dialog is placed.
    pub origin: (f32, f32),
    bounds: Rect,
    focus: Option<usize>,
    pressed: Option<usize>,
    /// An open pop-up menu: the item it belongs to and the menu.
    popup: Option<DialogPopup>,
    /// Dragging the dialog by its title bar.
    drag: Option<(f32, f32)>,
    /// A close box at the left of the title bar, for windows like Style
    /// and Preferences that have one instead of (or as well as) Cancel.
    pub close_box: bool,
    /// A window that lets the document be used while it is open (Find,
    /// Glossary, Spelling).
    pub modeless: bool,
    /// Drawn as inactive: a modeless window while the document has the
    /// keyboard.
    pub inactive: bool,
    /// The slider being dragged.
    held_slider: Option<usize>,
    /// A panel popped out beside an item, and the last box pressed in it.
    pub popout: Option<Popout>,
    pop_click: Option<(std::time::Instant, &'static str)>,
    /// The last row pressed in a list, to see a double-click.
    list_click: Option<(std::time::Instant, &'static str, usize)>,
    /// A plain box with no title bar, like Go To and alerts.
    pub plain: bool,
}

#[derive(Clone, Debug)]
struct PopupChoice {
    option: usize,
    variant: Option<FontVariant>,
}

struct DialogPopup {
    item: usize,
    menu: Menu,
    open: OpenMenu,
    submenu: Option<(usize, OpenMenu)>,
    choices: Vec<PopupChoice>,
    /// The press which opened a modern pop-up must not also choose the row
    /// underneath it when that same press is released.
    just_opened: bool,
}

impl Dialog {
    pub fn new(title: &str, width: f32, height: f32) -> Dialog {
        Dialog {
            title: title.to_string(),
            native_title: None,
            width,
            height,
            items: Vec::new(),
            origin: (0.0, 0.0),
            bounds: Rect::new(0.0, 0.0, 4000.0, 4000.0),
            focus: None,
            pressed: None,
            popup: None,
            drag: None,
            close_box: false,
            modeless: false,
            inactive: false,
            held_slider: None,
            popout: None,
            pop_click: None,
            list_click: None,
            plain: false,
        }
    }

    /// An alert: a plain box with a message and buttons, the first of
    /// which is the default.
    pub fn alert(message: &str, buttons: &[(&'static str, &str)]) -> Dialog {
        let lines = wrap(message, 40);
        let h = 24.0 + 16.0 * lines.len().max(2) as f32 + 34.0;
        let mut d = Dialog::new("", 360.0, h);
        d.plain = true;
        for (k, line) in lines.iter().enumerate() {
            d.text(line, 66.0, 14.0 + 16.0 * k as f32);
        }
        d.custom("alert_icon", 18.0, 14.0, 32.0, 32.0);
        let mut x = 360.0 - 16.0;
        for (k, (id, label)) in buttons.iter().enumerate() {
            let w = 68.0;
            x -= w;
            d.button(id, label, x, h - 32.0, w, k == 0);
            x -= 16.0;
        }
        d
    }

    fn title_height(&self) -> f32 {
        if self.plain { 0.0 } else { 28.0 }
    }

    /// Whether a point is on the dialog.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.frame().contains(x, y)
    }

    fn close_rect(&self) -> Rect {
        let f = self.frame();
        let y = 8.0;
        Rect::new(f.x + 8.0, f.y + y, 12.0, 12.0)
    }

    fn has_close_box(&self) -> bool {
        self.close_box || !self.plain
    }

    /// Make a checkbox three-state and set it.
    pub fn set_check3(&mut self, id: &str, state: Option<bool>) {
        if let Some(Kind::Check { on, tri, mixed }) = self.item_mut(id).map(|i| &mut i.kind) {
            *tri = true;
            *mixed = state.is_none();
            *on = state == Some(true);
        }
    }

    pub fn check3_state(&self, id: &str) -> Option<bool> {
        match self.item(id).map(|i| &i.kind) {
            Some(Kind::Check { mixed: true, .. }) | None => None,
            Some(Kind::Check { on, .. }) => Some(*on),
            _ => None,
        }
    }

    /// A menu button: a pop-up that always shows `title` and reports the
    /// chosen item as its selection.
    pub fn menu_button(
        &mut self,
        id: &'static str,
        title: &str,
        options: &[&str],
        x: f32,
        y: f32,
        w: f32,
    ) {
        self.popup(id, options, usize::MAX, x, y, w);
        if let Some(i) = self.item_mut(id) {
            i.label = title.to_string();
        }
    }

    /// Small interface text, with an id so it can be changed.
    pub fn note(&mut self, id: &'static str, label: &str, x: f32, y: f32, w: f32) {
        self.items
            .push(Item::new(id, label, Rect::new(x, y, w, 12.0), Kind::Note));
    }

    /// Change a text item's words.
    pub fn set_label(&mut self, id: &str, label: &str) {
        if let Some(i) = self.item_mut(id) {
            i.label = label.to_string();
        }
    }

    pub fn set_struck(&mut self, id: &str, struck: bool) {
        if let Some(item) = self.item_mut(id) {
            item.struck = struck;
        }
    }

    /// The displayed label for an item, when it exists.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn item_label(&self, id: &str) -> &str {
        self.item(id).map_or("", |item| item.label.as_str())
    }

    /// Type `text` into a field over its selection, and give it the focus.
    pub fn insert_in_field(&mut self, id: &str, text: &str) {
        let Some(k) = self.items.iter().position(|i| i.id == id) else {
            return;
        };
        if let Kind::Field { text: t, sel } = &mut self.items[k].kind {
            let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
            t.replace_range(a..b, text);
            *sel = (a + text.len(), a + text.len());
            self.focus = Some(k);
        }
    }

    /// Whether one of the dialog's pop-up menus is open.
    pub fn popup_open(&self) -> bool {
        self.popup.is_some()
    }

    /// An item's rectangle on screen.
    #[cfg(test)]
    pub fn item_rect(&self, id: &str) -> Option<Rect> {
        self.item(id).map(|i| self.at(i.rect))
    }

    fn item_mut(&mut self, id: &str) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    // ----- building -----

    pub fn button(&mut self, id: &'static str, label: &str, x: f32, y: f32, w: f32, default: bool) {
        self.items.push(Item::new(
            id,
            label,
            Rect::new(x, y, w, 20.0),
            Kind::Button { default },
        ));
    }

    pub fn check(&mut self, id: &'static str, label: &str, x: f32, y: f32, on: bool) {
        self.items.push(Item::new(
            id,
            label,
            Rect::new(x, y, label_width(label), 16.0),
            Kind::Check {
                on,
                tri: false,
                mixed: false,
            },
        ));
    }

    pub fn radio(&mut self, id: &'static str, label: &str, x: f32, y: f32, group: u8, on: bool) {
        self.items.push(Item::new(
            id,
            label,
            Rect::new(x, y, label_width(label), 16.0),
            Kind::Radio { group, on },
        ));
    }

    pub fn field(&mut self, id: &'static str, text: &str, x: f32, y: f32, w: f32) {
        let n = text.len();
        self.items.push(Item::new(
            id,
            "",
            Rect::new(x, y, w, 20.0),
            Kind::Field {
                text: text.to_string(),
                sel: (0, n),
            },
        ));
        if self.focus.is_none() {
            self.focus = Some(self.items.len() - 1);
        }
    }

    pub fn popup(
        &mut self,
        id: &'static str,
        options: &[&str],
        selected: usize,
        x: f32,
        y: f32,
        w: f32,
    ) {
        self.popup_owned(
            id,
            options.iter().map(|s| s.to_string()).collect(),
            selected,
            x,
            y,
            w,
        );
    }

    pub fn popup_owned(
        &mut self,
        id: &'static str,
        options: Vec<String>,
        selected: usize,
        x: f32,
        y: f32,
        w: f32,
    ) {
        self.items.push(Item::new(
            id,
            "",
            Rect::new(x, y, w, 20.0),
            Kind::Popup {
                options,
                selected,
                font_families: None,
            },
        ));
    }

    /// A font-family pop-up whose visible rows preview the installed face
    /// and whose submenus contain only variants actually present on disk.
    pub fn font_popup(
        &mut self,
        id: &'static str,
        families: &[FontFamilyInfo],
        selected: usize,
        x: f32,
        y: f32,
        w: f32,
    ) {
        self.popup_owned(
            id,
            families.iter().map(|f| f.name.clone()).collect(),
            selected,
            x,
            y,
            w,
        );
        if let Some(Item {
            kind: Kind::Popup { font_families, .. },
            ..
        }) = self.items.last_mut()
        {
            *font_families = Some(families.to_vec());
        }
    }

    pub fn text(&mut self, label: &str, x: f32, y: f32) {
        self.text_as("", label, x, y);
    }

    /// Static text with an id, so it can be dimmed or changed later.
    pub fn text_as(&mut self, id: &'static str, label: &str, x: f32, y: f32) {
        self.items.push(Item::new(
            id,
            label,
            Rect::new(x, y, 200.0, 16.0),
            Kind::Text,
        ));
    }

    pub fn group(&mut self, label: &str, x: f32, y: f32, w: f32, h: f32) {
        self.items
            .push(Item::new("", label, Rect::new(x, y, w, h), Kind::Group));
    }

    /// An area drawn by the dialog's owner; see `draw`.
    pub fn custom(&mut self, id: &'static str, x: f32, y: f32, w: f32, h: f32) {
        self.items
            .push(Item::new(id, "", Rect::new(x, y, w, h), Kind::Custom));
    }

    /// A box of the color `rgb` (0xRRGGBB).
    pub fn swatch(&mut self, id: &'static str, x: f32, y: f32, w: f32, h: f32, rgb: u32) {
        self.items.push(Item::new(
            id,
            "",
            Rect::new(x, y, w, h),
            Kind::Swatch { rgb, button: false },
        ));
    }

    /// A box of the color `rgb` that can be pressed, like a color well.
    pub fn swatch_button(&mut self, id: &'static str, x: f32, y: f32, w: f32, h: f32, rgb: u32) {
        self.items.push(Item::new(
            id,
            "",
            Rect::new(x, y, w, h),
            Kind::Swatch { rgb, button: true },
        ));
    }

    pub fn set_swatch(&mut self, id: &str, color: u32) {
        if let Some(Item {
            kind: Kind::Swatch { rgb, .. },
            ..
        }) = self.items.iter_mut().find(|i| i.id == id)
        {
            *rgb = color;
        }
    }

    /// The color of the swatch `id`.
    pub fn swatch_rgb(&self, id: &str) -> Option<u32> {
        match self.items.iter().find(|i| i.id == id).map(|i| &i.kind) {
            Some(Kind::Swatch { rgb, .. }) => Some(*rgb),
            _ => None,
        }
    }

    /// A slider from 0 to `max`, with its track going from color `from`
    /// A list of `rows` with `selected` chosen. A double-click on a row
    /// presses button `double`.
    #[allow(clippy::too_many_arguments)]
    pub fn list(
        &mut self,
        id: &'static str,
        rows: Vec<String>,
        selected: usize,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        double: Option<&'static str>,
    ) {
        let r = Rect::new(x, y, w, h);
        let mut state = crate::listbox::ListBox {
            len: rows.len(),
            ..Default::default()
        };
        state.select(selected, r);
        self.items.push(Item::new(
            id,
            "",
            r,
            Kind::List {
                rows,
                state,
                double,
            },
        ));
    }

    /// Put new rows in list `id`, with `selected` chosen.
    pub fn set_rows(&mut self, id: &str, new_rows: Vec<String>, selected: usize) {
        if let Some(item) = self.item_mut(id) {
            let r = item.rect;
            if let Kind::List { rows, state, .. } = &mut item.kind {
                state.len = new_rows.len();
                *rows = new_rows;
                state.select(selected, r);
            }
        }
    }

    /// The chosen row of list `id`, if it has any rows.
    pub fn list_selected(&self, id: &str) -> Option<usize> {
        match self.item(id).map(|i| &i.kind) {
            Some(Kind::List { rows, state, .. }) if !rows.is_empty() => {
                Some(state.selected.min(rows.len() - 1))
            }
            _ => None,
        }
    }

    /// Choose row `row` of list `id`, as a click on it does.
    #[cfg(any(target_os = "macos", test))]
    pub fn select_row(&mut self, id: &str, row: usize) {
        if let Some(item) = self.item_mut(id) {
            let r = item.rect;
            if let Kind::List { state, .. } = &mut item.kind {
                state.select(row, r);
            }
        }
    }

    /// A check box that is either on or off, never gray.
    pub fn set_two_state(&mut self, id: &str, state: bool) {
        if let Some(Kind::Check { on, tri, mixed }) = self.item_mut(id).map(|i| &mut i.kind) {
            *tri = false;
            *mixed = false;
            *on = state;
        }
    }

    /// to color `to`.
    #[allow(clippy::too_many_arguments)]
    pub fn slider(
        &mut self,
        id: &'static str,
        x: f32,
        y: f32,
        w: f32,
        max: i32,
        value: i32,
        (from, to): (u32, u32),
    ) {
        self.items.push(Item::new(
            id,
            "",
            Rect::new(x, y, w, 16.0),
            Kind::Slider {
                value: value.clamp(0, max),
                max,
                from,
                to,
            },
        ));
    }

    pub fn slider_value(&self, id: &str) -> i32 {
        match self.items.iter().find(|i| i.id == id).map(|i| &i.kind) {
            Some(Kind::Slider { value, .. }) => *value,
            _ => 0,
        }
    }

    pub fn set_slider(&mut self, id: &str, new: i32, track: (u32, u32)) {
        if let Some(Item {
            kind:
                Kind::Slider {
                    value,
                    max,
                    from,
                    to,
                },
            ..
        }) = self.items.iter_mut().find(|i| i.id == id)
        {
            *value = new.clamp(0, *max);
            (*from, *to) = track;
        }
    }

    /// Whether something in the dialog can only be drawn by Quill. A color
    /// button is not: native dialogs make it an ordinary button.
    #[cfg(target_os = "macos")]
    pub fn needs_canvas(&self) -> bool {
        self.items.iter().any(|i| {
            matches!(
                i.kind,
                Kind::Custom | Kind::Swatch { button: false, .. } | Kind::Slider { .. }
            )
        })
    }

    pub fn enable(&mut self, id: &str, on: bool) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.enabled = on;
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.item(id).is_some_and(|i| i.enabled)
    }

    /// Replace a popup's choices.
    /// A popup's choices.
    pub fn options(&self, id: &str) -> Vec<String> {
        match self.item(id).map(|i| &i.kind) {
            Some(Kind::Popup { options, .. }) => options.clone(),
            _ => Vec::new(),
        }
    }

    pub fn set_options(&mut self, id: &str, options: Vec<String>, selected: usize) {
        if let Some(Item {
            kind:
                Kind::Popup {
                    options: o,
                    selected: s,
                    font_families,
                },
            ..
        }) = self.items.iter_mut().find(|i| i.id == id)
        {
            *o = options;
            *s = selected;
            *font_families = None;
        }
    }

    /// A Bold and Italic choice in the font pop-up `id`: it sets the Bold
    /// and Italic boxes, in the dialogs that have them.
    pub fn set_font_variant(&mut self, bold: bool, italic: bool) {
        self.set_on("bold", bold);
        self.set_on("italic", italic);
    }

    pub fn set_selected(&mut self, id: &str, value: usize) {
        if let Some(Item {
            kind: Kind::Popup { selected, .. },
            ..
        }) = self.items.iter_mut().find(|i| i.id == id)
        {
            *selected = value;
        }
    }

    pub fn disable(&mut self, id: &str) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.enabled = false;
        }
    }

    /// Put keyboard focus in a field.
    pub fn focus(&mut self, id: &str) {
        self.focus = self
            .items
            .iter()
            .position(|i| i.id == id && i.takes_focus());
    }

    /// The field which should become first responder when a platform-native
    /// dialog is presented.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn focused_id(&self) -> Option<&'static str> {
        self.focus.map(|index| self.items[index].id)
    }

    // ----- reading values -----

    fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn is_on(&self, id: &str) -> bool {
        matches!(
            self.item(id).map(|i| &i.kind),
            Some(Kind::Check { on: true, .. } | Kind::Radio { on: true, .. })
        )
    }

    pub fn field_text(&self, id: &str) -> String {
        match self.item(id).map(|i| &i.kind) {
            Some(Kind::Field { text, .. }) => text.clone(),
            _ => String::new(),
        }
    }

    pub fn set_field(&mut self, id: &str, value: &str) {
        if let Some(Item {
            kind: Kind::Field { text, sel },
            ..
        }) = self.items.iter_mut().find(|i| i.id == id)
        {
            *text = value.to_string();
            *sel = (0, text.len());
        }
    }

    /// Turn a checkbox or radio button on or off (radio groups are not
    /// updated; turn the others off yourself).
    pub fn set_on(&mut self, id: &str, value: bool) {
        if let Some(it) = self.items.iter_mut().find(|i| i.id == id) {
            match &mut it.kind {
                Kind::Check { on, mixed, .. } => {
                    *on = value;
                    *mixed = false;
                }
                Kind::Radio { on, .. } => *on = value,
                _ => {}
            }
        }
    }

    pub fn selected(&self, id: &str) -> usize {
        match self.item(id).map(|i| &i.kind) {
            Some(Kind::Popup { selected, .. }) => *selected,
            _ => 0,
        }
    }

    pub fn selected_text(&self, id: &str) -> String {
        match self.item(id).map(|i| &i.kind) {
            Some(Kind::Popup {
                options, selected, ..
            }) => options.get(*selected).cloned().unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// The id of the radio button that is on in `group`.
    pub fn radio_on(&self, group: u8) -> &'static str {
        self.items
            .iter()
            .find(|i| matches!(i.kind, Kind::Radio { group: g, on: true } if g == group))
            .map_or("", |i| i.id)
    }

    /// Where an item's middle is on screen, for scripted clicks.
    pub fn item_center(&self, id: &str) -> Option<(f32, f32)> {
        let it = self.item(id)?;
        let r = self.at(it.rect);
        // Checkboxes and radio buttons: their box, not the label's end.
        let x = match it.kind {
            Kind::Check { .. } | Kind::Radio { .. } => r.x + 6.0,
            _ => r.x + r.w / 2.0,
        };
        Some((x, r.y + r.h / 2.0))
    }

    // ----- placement and drawing -----

    /// Center the dialog in the upper part of a window, as the Mac does.
    /// Center the dialog in the upper part of a window, as the Mac does,
    /// but no higher than `min_y` when the window has room (so the ruler
    /// stays in view above dialogs that change it).
    pub fn place(&mut self, window_w: f32, window_h: f32, min_y: f32) {
        self.bounds = Rect::new(0.0, 0.0, window_w, window_h);
        let w = self.width;
        let h = self.height + self.title_height();
        let mut y = ((window_h - h) / 3.0).max(24.0);
        // Modeless windows sit lower, clear of the text being worked on.
        if self.modeless {
            y = ((window_h - h) * 0.7).max(min_y).max(24.0);
        }
        if y < min_y && min_y + h <= window_h {
            y = min_y;
        }
        if self.title == "Find" && self.items.iter().any(|i| i.id == "mode") {
            y = ((window_h - h) / 2.0).max(0.0);
        }
        self.origin = (((window_w - w) / 2.0).max(0.0).round(), y.round());
    }

    fn frame(&self) -> Rect {
        Rect::new(
            self.origin.0,
            self.origin.1,
            self.width,
            self.height + self.title_height(),
        )
    }

    /// An item's rectangle on screen.
    fn at(&self, r: Rect) -> Rect {
        Rect::new(
            r.x + self.origin.0,
            r.y + self.origin.1 + self.title_height(),
            r.w,
            r.h,
        )
    }

    /// Draw the dialog. `custom` draws each custom item, given its id and
    /// rectangle on screen.
    pub fn draw(
        &self,
        c: &mut Canvas,
        label: &mut Label,
        custom: &mut dyn FnMut(&mut Canvas, &mut Label, &'static str, Rect),
    ) {
        let f = self.frame();
        c.panel_shadow(f, 9.0);
        modern_box(c, f, 9.0, 0xB8B8B8, 0xECECEC);
        if !self.plain {
            c.hline(f.x + 1.0, f.right() - 1.0, f.y + 27.0, 0xD0D0D0);
            let tw = label.width(MODERN_FONT, SIZE, false, &self.title);
            label.draw(
                c,
                MODERN_FONT,
                SIZE,
                false,
                f.x + (f.w - tw) / 2.0,
                f.y + 19.0,
                &self.title,
                if self.inactive { 0x777777 } else { 0x202020 },
            );
            if self.has_close_box() {
                let b = self.close_rect();
                label.draw(c, MODERN_FONT, 14.0, false, b.x, b.y + 11.0, "×", 0x555555);
            }
        }
        self.draw_contents(c, label, custom);
    }

    /// The items, and any open popup menu over them.
    fn draw_contents(
        &self,
        c: &mut Canvas,
        label: &mut Label,
        custom: &mut dyn FnMut(&mut Canvas, &mut Label, &'static str, Rect),
    ) {
        for (i, it) in self.items.iter().enumerate() {
            if matches!(it.kind, Kind::Custom) {
                custom(c, label, it.id, self.at(it.rect));
            } else {
                let focused = self.focus == Some(i) && !self.inactive;
                self.draw_item(c, label, it, focused, self.pressed == Some(i));
            }
        }
        self.draw_popout(c, label);
        if let Some(popup) = &self.popup {
            menu::draw_menu(c, label, &popup.menu, &popup.open);
            if let Some((parent, open)) = &popup.submenu
                && let Some(child) = popup.menu.items[*parent].submenu.as_ref()
            {
                menu::draw_menu(c, label, child, open);
            }
        }
    }

    /// Where the popped-out panel is: under its anchor, kept inside the
    /// dialog's width.
    fn popout_rect(&self) -> Option<Rect> {
        let p = self.popout.as_ref()?;
        let anchor = self.at(self.items.iter().find(|i| i.id == p.anchor)?.rect);
        let f = self.frame();
        let x = anchor.x.min(f.right() - p.size.0 - 8.0).max(f.x + 8.0);
        Some(Rect::new(x, anchor.bottom() + 2.0, p.size.0, p.size.1))
    }

    fn draw_popout(&self, c: &mut Canvas, label: &mut Label) {
        if let (Some(p), Some(panel)) = (&self.popout, self.popout_rect()) {
            draw_popout_panel(c, label, panel, p);
        }
    }

    fn draw_item(
        &self,
        c: &mut Canvas,
        label: &mut Label,
        it: &Item,
        focused: bool,
        pressed: bool,
    ) {
        let r = self.at(it.rect);
        self.draw_modern_item(c, label, it, r, focused, pressed);
    }

    fn draw_modern_item(
        &self,
        c: &mut Canvas,
        label: &mut Label,
        it: &Item,
        r: Rect,
        focused: bool,
        pressed: bool,
    ) {
        let fg = if it.enabled { 0x202020 } else { 0x8A8A8A };
        match &it.kind {
            Kind::Button { default } => {
                let fill = if *default {
                    if pressed { 0x0869D4 } else { 0x0A84FF }
                } else if pressed {
                    0xD1D1D1
                } else {
                    0xF7F7F7
                };
                if *default {
                    round_rect(c, r, 5.0, fill);
                } else {
                    modern_box(c, r, 5.0, 0xB6B6B6, fill);
                }
                let text_color = if *default { color::WHITE } else { fg };
                let w = label.width(MODERN_FONT, SIZE, false, &it.label);
                label.draw(
                    c,
                    MODERN_FONT,
                    SIZE,
                    false,
                    r.x + (r.w - w) / 2.0,
                    r.y + 14.5,
                    &it.label,
                    text_color,
                );
            }
            Kind::Check { on, mixed, .. } => {
                let b = Rect::new(r.x, r.y + 2.0, 13.0, 13.0);
                if *on || *mixed {
                    round_rect(c, b, 3.0, 0x0A84FF);
                } else {
                    modern_box(c, b, 3.0, 0xA8A8A8, 0xFFFFFF);
                }
                if *mixed {
                    c.fill(Rect::new(b.x + 3.0, b.y + 6.0, 7.0, 1.5), color::WHITE);
                } else {
                    for (x, y) in [
                        (3.0, 6.0),
                        (4.0, 7.0),
                        (5.0, 8.0),
                        (6.0, 7.0),
                        (7.0, 6.0),
                        (8.0, 5.0),
                        (9.0, 4.0),
                    ] {
                        c.fill(Rect::new(b.x + x, b.y + y - 0.5, 1.5, 2.0), color::WHITE);
                    }
                }
                label.draw(
                    c,
                    MODERN_FONT,
                    SIZE,
                    false,
                    r.x + 18.0,
                    r.y + 13.0,
                    &it.label,
                    fg,
                );
            }
            Kind::Radio { on, .. } => {
                let (cx, cy) = (r.x + 6.5, r.y + 8.5);
                c.circle(cx, cy, 6.5, if *on { 0x0A84FF } else { 0xA8A8A8 });
                c.circle(cx, cy, 5.0, if *on { 0x0A84FF } else { color::WHITE });
                if *on {
                    c.circle(cx, cy, 2.0, color::WHITE);
                }
                label.draw(
                    c,
                    MODERN_FONT,
                    SIZE,
                    false,
                    r.x + 18.0,
                    r.y + 13.0,
                    &it.label,
                    fg,
                );
            }
            Kind::Field { text, sel } => {
                modern_box(
                    c,
                    r,
                    4.0,
                    if focused { 0x0A84FF } else { 0xB3B3B3 },
                    if it.enabled { color::WHITE } else { 0xE4E4E4 },
                );
                let clip = c.set_clip(r.inset(3.0));
                let x0 = r.x + 5.0;
                if focused && it.enabled {
                    let before = label.width(MODERN_FONT, SIZE, false, &text[..sel.0]);
                    let mid = label.width(MODERN_FONT, SIZE, false, &text[sel.0..sel.1]);
                    if sel.0 == sel.1 {
                        c.vline(x0 + before, r.y + 3.0, r.bottom() - 3.0, 0x202020);
                    } else {
                        c.fill(Rect::new(x0 + before, r.y + 3.0, mid, r.h - 6.0), 0xBBD7FF);
                    }
                }
                label.draw(c, MODERN_FONT, SIZE, false, x0, r.y + 14.5, text, fg);
                c.restore_clip(clip);
            }
            Kind::Popup {
                options,
                selected,
                font_families,
            } => {
                modern_box(
                    c,
                    r,
                    5.0,
                    0xB3B3B3,
                    if it.enabled { 0xF8F8F8 } else { 0xE4E4E4 },
                );
                let text = if it.label.is_empty() {
                    options.get(*selected).map(String::as_str).unwrap_or("")
                } else {
                    it.label.as_str()
                };
                let preview = font_families.as_ref().and_then(|f| f.get(*selected));
                let clip = c.set_clip(Rect::new(r.x + 1.0, r.y, r.w - 22.0, r.h));
                label.draw(
                    c,
                    preview.map_or(MODERN_FONT, |f| f.name.as_str()),
                    SIZE,
                    false,
                    r.x + 7.0,
                    r.y + 14.5,
                    text,
                    fg,
                );
                c.restore_clip(clip);
                let cx = r.right() - 10.0;
                let cy = r.y + r.h / 2.0;
                for i in 0..4 {
                    let i = i as f32;
                    c.fill(
                        Rect::new(cx - 3.0 + i, cy - 1.5 + i, 7.0 - 2.0 * i, 1.0),
                        fg,
                    );
                }
            }
            Kind::Text => {
                label.draw(c, MODERN_FONT, SIZE, false, r.x, r.y + 12.5, &it.label, fg);
                if it.struck {
                    let width = label.width(MODERN_FONT, SIZE, false, &it.label).min(r.w);
                    c.hline(r.x, r.x + width, r.y + 8.0, fg);
                }
            }
            Kind::Note => {
                let clip = c.set_clip(r);
                label.draw(c, MODERN_FONT, 10.0, false, r.x, r.y + 10.5, &it.label, fg);
                if it.struck {
                    let width = label.width(MODERN_FONT, 10.0, false, &it.label).min(r.w);
                    c.hline(r.x, r.x + width, r.y + 7.0, fg);
                }
                c.restore_clip(clip);
            }
            Kind::Group => {
                c.hline(r.x, r.right(), r.y + 8.0, 0xC8C8C8);
                if !it.label.is_empty() {
                    let w = label.width(MODERN_FONT, SIZE, false, &it.label);
                    c.fill(Rect::new(r.x + 7.0, r.y, w + 8.0, 16.0), 0xECECEC);
                    label.draw(
                        c,
                        MODERN_FONT,
                        SIZE,
                        false,
                        r.x + 11.0,
                        r.y + 12.5,
                        &it.label,
                        fg,
                    );
                }
            }
            Kind::Custom => {}
            Kind::List { rows, state, .. } => {
                let rows: Vec<crate::listbox::Row> = rows
                    .iter()
                    .map(|text| crate::listbox::Row {
                        text,
                        mark: "",
                        bold: false,
                        underline: false,
                    })
                    .collect();
                state.draw(c, label, r, &rows);
            }
            Kind::Swatch { rgb, button } => {
                c.fill(r, *rgb);
                c.frame(
                    r,
                    if *button && pressed {
                        0x202020
                    } else {
                        0x808080
                    },
                );
            }
            Kind::Slider {
                value,
                max,
                from,
                to,
            } => {
                let track = Rect::new(r.x, r.y + 3.0, r.w, r.h - 6.0);
                // The track shades from one end color to the other.
                let steps = (track.w / 2.0).ceil().max(1.0) as usize;
                let mix = |a: u32, b: u32, t: f32| -> u32 {
                    let ch = |sh: u32| {
                        let (a, b) = (((a >> sh) & 0xFF) as f32, ((b >> sh) & 0xFF) as f32);
                        (a + (b - a) * t).round() as u32
                    };
                    ch(16) << 16 | ch(8) << 8 | ch(0)
                };
                for k in 0..steps {
                    let x = track.x + k as f32 * track.w / steps as f32;
                    let w = track.w / steps as f32 + 0.5;
                    let t = k as f32 / (steps - 1).max(1) as f32;
                    c.fill(
                        Rect::new(x, track.y, w.min(track.right() - x), track.h),
                        mix(*from, *to, t),
                    );
                }
                c.frame(track, 0x808080);
                let x = r.x + 4.0 + (r.w - 8.0) * *value as f32 / (*max).max(1) as f32;
                let knob = Rect::new((x - 3.0).round(), r.y, 7.0, r.h);
                c.fill(knob, 0xFFFFFF);
                c.frame(knob, 0x404040);
            }
        }
    }

    // ----- input -----

    fn item_at(&self, x: f32, y: f32) -> Option<usize> {
        self.items.iter().position(|it| {
            it.enabled
                && !matches!(
                    it.kind,
                    Kind::Text | Kind::Note | Kind::Group | Kind::Swatch { button: false, .. }
                )
                && self.at(it.rect).contains(x, y)
        })
    }

    pub fn mouse_down(&mut self, x: f32, y: f32, shift: bool, label: &mut Label) -> Outcome {
        if let Some(panel) = self.popout_rect() {
            if panel.contains(x, y) {
                let (lx, ly) = (x - panel.x, y - panel.y);
                let hit = self
                    .popout
                    .as_ref()
                    .and_then(|p| p.items.iter().find(|it| it.rect.contains(lx, ly)))
                    .filter(|it| it.enabled)
                    .map(|it| (it.id, !it.label.is_empty(), it.rgb.is_some()));
                return match hit {
                    Some((id, true, _)) => Outcome::Button(id),
                    Some((id, false, true)) => {
                        let now = std::time::Instant::now();
                        let double = self.pop_click.is_some_and(|(t, last)| {
                            last == id && now - t < std::time::Duration::from_millis(450)
                        });
                        self.pop_click = (!double).then_some((now, id));
                        Outcome::Swatch { id, double }
                    }
                    _ => Outcome::None,
                };
            }
            // A press anywhere else puts the panel away; one on the item it
            // came out of does no more than that.
            let anchor = self
                .popout
                .as_ref()
                .and_then(|p| self.items.iter().find(|i| i.id == p.anchor))
                .map(|i| self.at(i.rect));
            self.popout = None;
            if anchor.is_some_and(|a| a.contains(x, y)) {
                return Outcome::None;
            }
        }
        if let Some(popup) = &mut self.popup {
            let in_parent = popup.open.rect.contains(x, y);
            let in_child = popup
                .submenu
                .as_ref()
                .is_some_and(|(_, open)| open.rect.contains(x, y));
            if !in_parent && !in_child {
                self.popup = None;
            }
            return Outcome::None;
        }
        let f = self.frame();
        if self.has_close_box() && self.close_rect().contains(x, y) {
            return Outcome::Cancel;
        }
        if Rect::new(f.x, f.y, f.w, self.title_height()).contains(x, y) {
            self.drag = Some((x - self.origin.0, y - self.origin.1));
            return Outcome::None;
        }
        let Some(i) = self.item_at(x, y) else {
            return Outcome::None;
        };
        match &self.items[i].kind {
            Kind::Custom => {
                let r = self.at(self.items[i].rect);
                return Outcome::Clicked(self.items[i].id, x - r.x, y - r.y, shift);
            }
            Kind::Slider { .. } => {
                self.held_slider = Some(i);
                return self.drag_slider(x);
            }
            Kind::List { .. } => {
                let r = self.at(self.items[i].rect);
                let id = self.items[i].id;
                let Kind::List { state, double, .. } = &mut self.items[i].kind else {
                    unreachable!();
                };
                return match state.click(x - r.x, y - r.y, r) {
                    crate::listbox::Click::Chose(row) => {
                        let now = std::time::Instant::now();
                        let again = self.list_click.is_some_and(|(t, last, k)| {
                            last == id
                                && k == row
                                && now - t < std::time::Duration::from_millis(450)
                        });
                        self.list_click = (!again).then_some((now, id, row));
                        match double {
                            Some(button) if again => Outcome::Button(button),
                            _ => Outcome::Changed(id),
                        }
                    }
                    _ => Outcome::None,
                };
            }
            Kind::Popup {
                options,
                selected,
                font_families,
            } => {
                let mut choices = Vec::new();
                let items = if let Some(families) = font_families {
                    families
                        .iter()
                        .enumerate()
                        .map(|(k, family)| {
                            let normal = choices.len() as u32;
                            choices.push(PopupChoice {
                                option: k,
                                variant: Some(FontVariant {
                                    bold: false,
                                    italic: false,
                                }),
                            });
                            let mut item = MenuItem::new(family.name.clone(), normal)
                                .checked(k == *selected)
                                .font(family.name.clone());
                            if family.variants.len() > 1 {
                                let children = family
                                    .variants
                                    .iter()
                                    .map(|variant| {
                                        let id = choices.len() as u32;
                                        choices.push(PopupChoice {
                                            option: k,
                                            variant: Some(*variant),
                                        });
                                        let name = match (variant.bold, variant.italic) {
                                            (false, false) => "Normal",
                                            (true, false) => "Bold",
                                            (false, true) => "Italic",
                                            (true, true) => "Bold Italic",
                                        };
                                        MenuItem::new(name, id).font(family.name.clone()).style(
                                            menu::ItemStyle {
                                                bold: variant.bold,
                                                italic: variant.italic,
                                                ..Default::default()
                                            },
                                        )
                                    })
                                    .collect();
                                item = item.submenu(Menu {
                                    title: family.name.clone(),
                                    items: children,
                                });
                            }
                            item
                        })
                        .collect()
                } else {
                    options
                        .iter()
                        .enumerate()
                        .map(|(k, o)| {
                            choices.push(PopupChoice {
                                option: k,
                                variant: None,
                            });
                            MenuItem::new(o.clone(), k as u32).checked(k == *selected)
                        })
                        .collect()
                };
                let m = Menu {
                    title: String::new(),
                    items,
                };
                let r = self.at(self.items[i].rect);
                let open = menu::place(label, &m, r, self.bounds, true);
                self.popup = Some(DialogPopup {
                    item: i,
                    menu: m,
                    open,
                    submenu: None,
                    choices,
                    just_opened: true,
                });
            }
            Kind::Field { text, .. } => {
                let r = self.at(self.items[i].rect);
                let offset = hit_text(label, text, x - r.x - 4.0);
                self.focus = Some(i);
                if let Kind::Field { sel, .. } = &mut self.items[i].kind {
                    *sel = (offset, offset);
                }
            }
            _ => self.pressed = Some(i),
        }
        Outcome::None
    }

    /// Move the held slider to the pointer's x.
    fn drag_slider(&mut self, x: f32) -> Outcome {
        let Some(i) = self.held_slider else {
            return Outcome::None;
        };
        let r = self.at(self.items[i].rect);
        let id = self.items[i].id;
        let Kind::Slider { value, max, .. } = &mut self.items[i].kind else {
            return Outcome::None;
        };
        let t = ((x - r.x - 4.0) / (r.w - 8.0).max(1.0)).clamp(0.0, 1.0);
        let new = (t * *max as f32).round() as i32;
        if new == *value {
            return Outcome::None;
        }
        *value = new;
        Outcome::Changed(id)
    }

    pub fn mouse_move(&mut self, x: f32, y: f32, label: &mut Label) -> Outcome {
        if let Some((dx, dy)) = self.drag {
            self.origin = ((x - dx).round(), (y - dy).max(20.0).round());
        }
        if self.held_slider.is_some() {
            return self.drag_slider(x);
        }
        if let Some(popup) = &mut self.popup {
            if let Some((parent, child_open)) = &mut popup.submenu
                && let Some(child) = popup.menu.items[*parent].submenu.as_ref()
                && child_open.rect.contains(x, y)
            {
                child_open.hot = menu::item_at(child, child_open, x, y);
                popup.open.hot = Some(*parent);
                return Outcome::None;
            }
            popup.open.hot = menu::item_at(&popup.menu, &popup.open, x, y);
            if let Some(k) = popup.open.hot
                && let Some(child) = popup.menu.items[k].submenu.as_ref()
                && let Some(parent) = menu::item_rect(&popup.menu, &popup.open, k)
            {
                if popup.submenu.as_ref().map(|s| s.0) != Some(k) {
                    popup.submenu = Some((k, menu::place_side(label, child, parent, self.bounds)));
                }
            } else {
                popup.submenu = None;
            }
        }
        Outcome::None
    }

    /// Scroll an open pop-up with a wheel or trackpad. The submenu under
    /// the pointer gets the gesture; otherwise the family list does.
    pub fn wheel(&mut self, x: f32, y: f32, rows: i32) -> bool {
        let Some(popup) = &mut self.popup else {
            return false;
        };
        if let Some((parent, open)) = &mut popup.submenu
            && let Some(child) = popup.menu.items[*parent].submenu.as_ref()
            && open.rect.contains(x, y)
        {
            if menu::scroll_by(child, open, rows) {
                open.hot = menu::item_at(child, open, x, y);
            }
            return true;
        }
        if menu::scroll_by(&popup.menu, &mut popup.open, rows) {
            popup.open.hot = menu::item_at(&popup.menu, &popup.open, x, y);
        }
        true
    }

    pub fn mouse_up(&mut self, x: f32, y: f32) -> Outcome {
        self.drag = None;
        if self.held_slider.take().is_some() {
            return Outcome::None;
        }
        if let Some(popup) = &mut self.popup {
            if popup.just_opened {
                popup.just_opened = false;
                return Outcome::None;
            }
            let choice = popup
                .submenu
                .as_ref()
                .and_then(|(parent, open)| {
                    popup.menu.items[*parent]
                        .submenu
                        .as_ref()
                        .and_then(|child| {
                            menu::item_at(child, open, x, y).map(|k| child.items[k].id)
                        })
                })
                .or_else(|| {
                    menu::item_at(&popup.menu, &popup.open, x, y).map(|k| popup.menu.items[k].id)
                });
            let Some(choice) = choice.and_then(|id| popup.choices.get(id as usize).cloned()) else {
                return Outcome::None;
            };
            let item = popup.item;
            if let Kind::Popup { selected, .. } = &mut self.items[item].kind {
                *selected = choice.option;
            }
            let id = self.items[item].id;
            if let Some(variant) = choice.variant {
                self.set_font_variant(variant.bold, variant.italic);
            }
            self.popup = None;
            return Outcome::Changed(id);
        }
        let Some(i) = self.pressed.take() else {
            return Outcome::None;
        };
        if !self.at(self.items[i].rect).contains(x, y) {
            return Outcome::None;
        }
        self.activate(i)
    }

    fn activate(&mut self, i: usize) -> Outcome {
        let id = self.items[i].id;
        match &mut self.items[i].kind {
            Kind::Check { on, tri, mixed } => {
                // Three-state boxes go gray, checked, clear, gray.
                if *tri && *mixed {
                    *mixed = false;
                    *on = true;
                } else if *tri && !*on {
                    *mixed = true;
                } else {
                    *on = !*on;
                }
                Outcome::Changed(id)
            }
            Kind::Radio { group, .. } => {
                let g = *group;
                for it in &mut self.items {
                    if let Kind::Radio { group, on } = &mut it.kind
                        && *group == g
                    {
                        *on = it.id == id;
                    }
                }
                Outcome::Changed(id)
            }
            Kind::Swatch { button: true, .. } => Outcome::Button(id),
            Kind::Button { .. } => match id {
                "ok" => Outcome::Ok,
                "cancel" => Outcome::Cancel,
                "apply" => Outcome::Apply,
                _ => Outcome::Button(id),
            },
            _ => Outcome::None,
        }
    }

    /// A key press. `text` is what the key types, if anything.
    pub fn key(&mut self, key: DKey, text: Option<&str>) -> Outcome {
        if self.popup.is_some() {
            if key == DKey::Escape {
                self.popup = None;
            }
            return Outcome::None;
        }
        match key {
            DKey::Escape => return Outcome::Cancel,
            DKey::Return => {
                // Return presses the default button, if it can be pressed.
                return match self
                    .items
                    .iter()
                    .position(|i| matches!(i.kind, Kind::Button { default: true }))
                {
                    Some(k) if self.items[k].enabled => self.activate(k),
                    Some(_) => Outcome::None,
                    None => Outcome::Ok,
                };
            }
            DKey::Tab { back } => {
                let fields: Vec<usize> = (0..self.items.len())
                    .filter(|&i| self.items[i].takes_focus())
                    .collect();
                if fields.is_empty() {
                    return Outcome::None;
                }
                let cur = self
                    .focus
                    .and_then(|f| fields.iter().position(|&x| x == f))
                    .unwrap_or(0);
                let next = if back {
                    (cur + fields.len() - 1) % fields.len()
                } else {
                    (cur + 1) % fields.len()
                };
                self.focus = Some(fields[next]);
                if let Kind::Field { text, sel } = &mut self.items[fields[next]].kind {
                    *sel = (0, text.len());
                }
                return Outcome::None;
            }
            _ => {}
        }
        let Some(f) = self.focus else {
            return Outcome::None;
        };
        let id = self.items[f].id;
        let Kind::Field { text: field, sel } = &mut self.items[f].kind else {
            return Outcome::None;
        };
        let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
        let prev = |s: &str, i: usize| s[..i].chars().next_back().map_or(0, |c| i - c.len_utf8());
        let next = |s: &str, i: usize| s[i..].chars().next().map_or(i, |c| i + c.len_utf8());
        match key {
            DKey::Backspace => {
                let start = if a == b { prev(field, a) } else { a };
                field.replace_range(start..b, "");
                *sel = (start, start);
            }
            DKey::Delete => {
                let end = if a == b { next(field, b) } else { b };
                field.replace_range(a..end, "");
                *sel = (a, a);
            }
            DKey::Left => {
                let p = if a == b { prev(field, a) } else { a };
                *sel = (p, p);
            }
            DKey::Right => {
                let p = if a == b { next(field, b) } else { b };
                *sel = (p, p);
            }
            DKey::SelectAll => *sel = (0, field.len()),
            DKey::Char => {
                let Some(t) = text else {
                    return Outcome::None;
                };
                let t: String = t.chars().filter(|c| !c.is_control()).collect();
                if t.is_empty() {
                    return Outcome::None;
                }
                field.replace_range(a..b, &t);
                *sel = (a + t.len(), a + t.len());
            }
            _ => return Outcome::None,
        }
        Outcome::Changed(id)
    }
}

/// Keys a dialog understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DKey {
    Char,
    Backspace,
    Delete,
    Left,
    Right,
    Tab { back: bool },
    Return,
    Escape,
    SelectAll,
}

/// Click area of a checkbox or radio button: the box and its label, using
/// the bold system font's average width. Too wide and neighboring
/// columns would steal each other's clicks.
/// Break text into lines of at most about `n` characters, at spaces.
pub fn wrap(text: &str, n: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > n {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn label_width(label: &str) -> f32 {
    18.0 + 7.5 * label.chars().count() as f32
}

/// Byte offset in `text` nearest to `x` points from its start.
fn hit_text(label: &mut Label, text: &str, x: f32) -> usize {
    let mut best = 0;
    for (i, _) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        let w = label.width(MODERN_FONT, SIZE, false, &text[..i]);
        if w <= x + 2.0 {
            best = i;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_popup(d: &mut Dialog, count: usize) {
        let menu = Menu {
            title: String::new(),
            items: (0..count)
                .map(|i| MenuItem::new(format!("Choice {i}"), i as u32))
                .collect(),
        };
        d.popup = Some(DialogPopup {
            item: 0,
            menu,
            open: OpenMenu {
                rect: Rect::new(10.0, 10.0, 120.0, 52.0),
                visible_rows: 3,
                ..Default::default()
            },
            submenu: None,
            choices: (0..count)
                .map(|option| PopupChoice {
                    option,
                    variant: None,
                })
                .collect(),
            just_opened: true,
        });
    }

    #[test]
    fn popup_stays_open_after_the_click_that_opened_it() {
        let mut d = Dialog::new("", 200.0, 100.0);
        d.popup("choice", &["One", "Two", "Three"], 0, 0.0, 0.0, 100.0);
        open_popup(&mut d, 3);

        assert_eq!(d.mouse_up(20.0, 36.0), Outcome::None);
        assert!(d.popup_open());
        assert_eq!(d.mouse_up(20.0, 36.0), Outcome::Changed("choice"));
        assert_eq!(d.selected("choice"), 1);
        assert!(!d.popup_open());
    }

    #[test]
    fn wheel_scrolls_an_open_dialog_popup() {
        let mut d = Dialog::new("", 200.0, 100.0);
        let choices: Vec<String> = (0..100).map(|i| format!("Choice {i}")).collect();
        d.popup_owned("choice", choices, 0, 0.0, 0.0, 100.0);
        open_popup(&mut d, 100);

        assert!(d.wheel(20.0, 20.0, 5));
        assert_eq!(d.popup.as_ref().unwrap().open.scroll, 5);
    }

    #[test]
    fn font_variant_choice_updates_the_character_style() {
        let mut d = Dialog::new("", 200.0, 100.0);
        d.popup_owned("font", vec!["Arial".into()], 0, 0.0, 0.0, 100.0);
        d.check("bold", "Bold", 0.0, 30.0, false);
        d.check("italic", "Italic", 0.0, 50.0, false);
        let child = Menu {
            title: "Arial".into(),
            items: vec![MenuItem::new("Bold Italic", 1)],
        };
        d.popup = Some(DialogPopup {
            item: 0,
            menu: Menu {
                title: String::new(),
                items: vec![MenuItem::new("Arial", 0).submenu(child)],
            },
            open: OpenMenu {
                rect: Rect::new(10.0, 10.0, 120.0, 20.0),
                visible_rows: 1,
                ..Default::default()
            },
            submenu: Some((
                0,
                OpenMenu {
                    rect: Rect::new(129.0, 10.0, 100.0, 20.0),
                    visible_rows: 1,
                    ..Default::default()
                },
            )),
            choices: vec![
                PopupChoice {
                    option: 0,
                    variant: Some(FontVariant {
                        bold: false,
                        italic: false,
                    }),
                },
                PopupChoice {
                    option: 0,
                    variant: Some(FontVariant {
                        bold: true,
                        italic: true,
                    }),
                },
            ],
            just_opened: false,
        });

        assert_eq!(d.mouse_up(150.0, 15.0), Outcome::Changed("font"));
        assert!(d.is_on("bold"));
        assert!(d.is_on("italic"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn every_titled_mac_dialog_has_a_close_control() {
        let dialog = Dialog::new("Character", 200.0, 100.0);
        assert!(dialog.has_close_box());

        let alert = Dialog::alert("Something happened.", &[("ok", "OK")]);
        assert!(!alert.has_close_box());
    }
}

/// Measurements in dialogs: lengths are shown in the user's unit and typed
/// in any unit the original understood.
pub mod units {
    use quill_core::Twips;

    /// Parse "1.25 in", "1.25\"", "12 pt", "3 cm", "1p6", "6 li" (lines of
    /// 12 points). A bare number means `default_unit` ("in" or "pt").
    pub fn parse(s: &str, default_unit: &str) -> Option<Twips> {
        let s = s.trim().to_ascii_lowercase();
        if s.is_empty() {
            return Some(0);
        }
        let split = s
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
            .unwrap_or(s.len());
        let (num, unit) = s.split_at(split);
        let n: f64 = num.parse().ok()?;
        let unit = unit.trim();
        let unit = if unit.is_empty() { default_unit } else { unit };
        let per = match unit {
            "in" | "\"" | "inch" | "inches" => 1440.0,
            "pt" | "pts" | "point" | "points" => 20.0,
            "cm" => 1440.0 / 2.54,
            "mm" => 1440.0 / 25.4,
            "pi" | "p" | "pica" | "picas" => 240.0,
            "li" | "line" | "lines" => 240.0,
            _ => return None,
        };
        // Refuse anything past a million twips (about 700 inches) instead
        // of letting the cast saturate; i32::MIN has no absolute value.
        let t = (n * per).round();
        if !t.is_finite() || t.abs() > 1_000_000.0 {
            return None;
        }
        Some(t as Twips)
    }

    /// A length in the unit chosen in Preferences: "1.25 in", "3.17 cm".
    pub fn length(t: Twips) -> String {
        let unit = crate::settings::with(|s| s.unit);
        let per = per_unit(unit.abbrev());
        format!("{} {}", trim(t as f64 / per), unit.abbrev())
    }

    /// Parse a length typed in a dialog; a bare number is in the unit
    /// chosen in Preferences.
    pub fn parse_length(s: &str) -> Option<Twips> {
        parse(s, crate::settings::with(|s| s.unit).abbrev())
    }

    /// Read a length field that was filled in from `old`. While it still
    /// says what `length(old)` showed, `old` itself comes back: the shown
    /// text is rounded, and re-reading it would change a measurement
    /// nobody edited (12 pt shows as 0.17 in, which reads back as 12.25 pt).
    pub fn parse_length_from(s: &str, old: Twips) -> Option<Twips> {
        if s.trim() == length(old) {
            Some(old)
        } else {
            parse_length(s)
        }
    }

    fn per_unit(unit: &str) -> f64 {
        match unit {
            "cm" => 1440.0 / 2.54,
            "mm" => 1440.0 / 25.4,
            "pt" => 20.0,
            "pi" => 240.0,
            _ => 1440.0,
        }
    }

    /// Points the way the original shows them: "12 pt".
    pub fn points(t: Twips) -> String {
        format!("{} pt", trim(t as f64 / 20.0))
    }

    fn trim(v: f64) -> String {
        let s = format!("{v:.2}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s == "-0" { "0".into() } else { s.to_string() }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_word_units() {
            assert_eq!(parse("1.25 in", "in"), Some(1800));
            assert_eq!(parse("1.25\"", "in"), Some(1800));
            assert_eq!(parse("12 pt", "in"), Some(240));
            assert_eq!(parse("25.4 mm", "in"), Some(1440));
            // Unedited text gives back the exact value it was shown from.
            assert_eq!(length(240), "0.17 in");
            assert_eq!(parse_length_from("0.17 in", 240), Some(240));
            assert_eq!(parse_length_from(" 0.17 in ", 240), Some(240));
            assert_eq!(parse_length_from("0.17", 240), Some(245));
            assert_eq!(parse_length_from("0.25 in", 240), Some(360));
            assert_eq!(parse("1.5 pica", "in"), Some(360));
            assert_eq!(parse("1.5 picas", "in"), Some(360));
            assert_eq!(parse("25.4", "mm"), Some(1440));
            assert_eq!(parse("12", "pt"), Some(240));
            assert_eq!(parse("2.54cm", "in"), Some(1440));
            assert_eq!(parse("-0.5 in", "in"), Some(-720));
            assert_eq!(parse("", "in"), Some(0));
            assert_eq!(parse("abc", "in"), None);
        }

        #[test]
        fn rejects_out_of_range_lengths() {
            for text in [
                "-999999999999 pt",
                "999999999999 pt",
                "99999999999999999999999999999999999999 in",
                "-99999999999999999999999999999999999999",
                "1000000 in",
            ] {
                assert_eq!(parse(text, "in"), None, "{text}");
            }
            assert_eq!(parse("-1000000", "pt"), None);
            assert_eq!(parse("1e5 pt", "in"), None);
            assert_eq!(parse("\u{ff11}\u{ff12} pt", "in"), None);
            assert_eq!(parse("-1.0 in", "in"), Some(-1440));
            // The limit itself is still accepted.
            assert_eq!(parse("50000 pt", "in"), Some(1_000_000));
        }

        #[test]
        fn formats_like_word() {
            assert_eq!(length(1800), "1.25 in");
            assert_eq!(length(0), "0 in");
            assert_eq!(length(720), "0.5 in");
            assert_eq!(points(240), "12 pt");
            assert_eq!(points(30), "1.5 pt");
        }
    }
}
