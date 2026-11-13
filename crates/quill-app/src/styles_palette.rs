//! The Style Sheets palette: one list of the document's paragraph and
//! character styles, with tabs to show All, only Paragraph or only
//! Character styles, and a row of buttons to make, change, copy and delete
//! them. It docks against the right side of the document window, or comes
//! out into a small window of its own that can go anywhere on the screen.

use crate::canvas::{Canvas, Label, Rect, color};
use quill_core::Document;
use quill_core::props::StyleId;

/// Heights and widths in interface units.
pub const TITLE: f32 = 20.0;
const TABS: f32 = 26.0;
const BUTTONS: f32 = 26.0;
const ROW: f32 = 20.0;
/// The docked width, and the size of the window it comes out into.
pub const DOCK_WIDTH: f32 = 220.0;
pub const WINDOW_SIZE: (f32, f32) = (220.0, 340.0);
/// The smallest the palette's own window goes.
pub const WINDOW_MIN: (f32, f32) = (170.0, 160.0);
/// The width of the dock button in the tab row of the palette's own window.
const DOCK_BUTTON: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    All,
    Paragraph,
    Character,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::All, Tab::Paragraph, Tab::Character];

    pub fn name(self) -> &'static str {
        match self {
            Tab::All => "All",
            Tab::Paragraph => "Paragraph",
            Tab::Character => "Character",
        }
    }

    /// For a palette too narrow for the whole name.
    fn short(self) -> &'static str {
        match self {
            Tab::All => "All",
            Tab::Paragraph => "Para",
            Tab::Character => "Char",
        }
    }

    pub fn index(self) -> u8 {
        self as u8
    }

    pub fn from_index(i: u8) -> Tab {
        match i {
            1 => Tab::Paragraph,
            2 => Tab::Character,
            _ => Tab::All,
        }
    }
}

/// One row of the list. `Character(0)` is the character styles' "No Style".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// The paragraph styles' "No Style": the selection goes back to Normal
    /// and loses its character styles, so styles can be applied afresh.
    NoParagraph,
    /// Clear Formatting for paragraphs: plain Normal, nothing set by hand.
    ClearParagraph,
    Paragraph(StyleId),
    Character(u16),
    /// Clear Formatting for text: no character styles, nothing set by hand.
    ClearCharacter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    New,
    Edit,
    Copy,
    Delete,
}

const BUTTON_LIST: [(Button, &str); 4] = [
    (Button::New, "New"),
    (Button::Edit, "Edit"),
    (Button::Copy, "Copy"),
    (Button::Delete, "Delete"),
];

/// What a click on the palette hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Close,
    /// Dock or undock.
    Dock,
    Tab(Tab),
    Button(Button),
    Row(usize),
    /// Palette furniture that does nothing.
    Chrome,
}

pub struct Palette {
    pub tab: Tab,
    /// Docked to the right of the document, not in a window of its own.
    pub docked: bool,
    /// While it is drawn in a window of its own, that window's size in
    /// interface units. The operating system's title bar is then the
    /// palette's, so it draws none.
    pub window: Option<(f32, f32)>,
    pub scroll: f32,
    /// The row last clicked, which Edit, Copy and Delete work on.
    pub chosen: Option<Entry>,
    pub hover: Option<Hit>,
}

impl Palette {
    pub fn new(tab: Tab, docked: bool) -> Palette {
        Palette {
            tab,
            docked,
            window: None,
            scroll: 0.0,
            chosen: None,
            hover: None,
        }
    }

    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.scroll = 0.0;
    }
}

/// The rows the tab shows. Paragraph styles come first: "No Style", Normal
/// and the rest sorted by name. Then the character styles: "No Style" and
/// the rest sorted by name.
pub fn entries(doc: &Document, tab: Tab) -> Vec<Entry> {
    let mut rows = Vec::new();
    if tab != Tab::Character {
        let mut ids: Vec<StyleId> = (1..doc.styles.styles.len() as StyleId).collect();
        ids.sort_by_key(|&id| doc.styles.get(id).display_name().to_lowercase());
        rows.push(Entry::NoParagraph);
        rows.push(Entry::ClearParagraph);
        rows.push(Entry::Paragraph(0));
        rows.extend(ids.into_iter().map(Entry::Paragraph));
    }
    if tab != Tab::Paragraph {
        let mut ids: Vec<u16> = (1..=doc.styles.characters.len() as u16).collect();
        ids.sort_by_key(|&id| {
            doc.styles
                .character(id)
                .map_or(String::new(), |s| s.name.to_lowercase())
        });
        rows.push(Entry::Character(0));
        rows.push(Entry::ClearCharacter);
        rows.extend(ids.into_iter().map(Entry::Character));
    }
    rows
}

/// What the row reads.
pub fn name(doc: &Document, e: Entry) -> String {
    match e {
        Entry::NoParagraph => "No Style".to_string(),
        Entry::ClearParagraph | Entry::ClearCharacter => "Clear Formatting".to_string(),
        Entry::Paragraph(id) => doc.styles.get(id).display_name(),
        Entry::Character(0) => "No Style".to_string(),
        Entry::Character(id) => doc
            .styles
            .character(id)
            .map_or(String::new(), |s| s.name.clone()),
    }
}

/// Whether Edit and Copy have something to work on: every style but
/// "No Style".
pub fn can_edit(e: Option<Entry>) -> bool {
    !matches!(
        e,
        None | Some(Entry::Character(0))
            | Some(Entry::NoParagraph)
            | Some(Entry::ClearParagraph)
            | Some(Entry::ClearCharacter)
    )
}

/// Whether the style can be deleted: not Normal, an automatic style or
/// "No Style".
pub fn can_delete(doc: &Document, e: Option<Entry>) -> bool {
    match e {
        Some(Entry::Paragraph(id)) => id != 0 && !doc.styles.get(id).builtin,
        Some(Entry::Character(id)) => id != 0,
        Some(Entry::NoParagraph | Entry::ClearParagraph | Entry::ClearCharacter) | None => false,
    }
}

struct Layout {
    title: Rect,
    tabs: Rect,
    buttons: Rect,
    list: Rect,
    /// The close box, in the title bar of the docked palette.
    close: Option<Rect>,
    dock: Rect,
}

/// Whether the system puts the close button at the left of a title bar.
const CLOSE_AT_LEFT: bool = cfg!(target_os = "macos");

/// `os`: the palette is in a window of its own, whose title bar the system
/// draws, with its close button. The dock button then sits in the tab row.
fn layout(b: Rect, os: bool) -> Layout {
    let title = Rect::new(b.x, b.y, b.w, if os { 0.0 } else { TITLE });
    let mut tabs = Rect::new(b.x, title.bottom(), b.w, TABS);
    let buttons = Rect::new(b.x, tabs.bottom(), b.w, BUTTONS);
    let list = Rect::new(
        b.x,
        buttons.bottom(),
        b.w,
        (b.bottom() - buttons.bottom()).max(0.0),
    );
    let (close, dock) = if os {
        let dock = Rect::new(
            tabs.right() - DOCK_BUTTON - 2.0,
            tabs.y + 5.0,
            DOCK_BUTTON,
            TABS - 6.0,
        );
        tabs.w -= DOCK_BUTTON + 2.0;
        (None, dock)
    } else if CLOSE_AT_LEFT {
        (
            Some(Rect::new(title.x + 6.0, title.y + 4.0, 12.0, 12.0)),
            Rect::new(title.right() - 22.0, title.y + 1.0, 20.0, TITLE - 2.0),
        )
    } else {
        (
            Some(Rect::new(
                title.right() - 20.0,
                title.y + 1.0,
                18.0,
                TITLE - 2.0,
            )),
            Rect::new(title.x + 2.0, title.y + 1.0, 20.0, TITLE - 2.0),
        )
    };
    Layout {
        title,
        tabs,
        buttons,
        list,
        close,
        dock,
    }
}

fn tab_box(tabs: Rect, i: usize) -> Rect {
    let w = (tabs.w - 8.0) / 3.0;
    Rect::new(tabs.x + 4.0 + w * i as f32, tabs.y + 4.0, w, TABS - 4.0)
}

fn button_box(buttons: Rect, i: usize) -> Rect {
    let w = (buttons.w - 8.0) / BUTTON_LIST.len() as f32;
    Rect::new(
        buttons.x + 4.0 + w * i as f32,
        buttons.y + 3.0,
        w - 2.0,
        BUTTONS - 6.0,
    )
}

/// How far the list can scroll with `rows` rows.
pub fn max_scroll(b: Rect, os: bool, rows: usize) -> f32 {
    (rows as f32 * ROW - layout(b, os).list.h).max(0.0)
}

/// What is at (`x`, `y`) of a palette in `b`, or None outside it.
pub fn hit(b: Rect, os: bool, scroll: f32, rows: usize, x: f32, y: f32) -> Option<Hit> {
    if !b.contains(x, y) {
        return None;
    }
    let l = layout(b, os);
    if l.dock.contains(x, y) {
        return Some(Hit::Dock);
    }
    if l.title.contains(x, y) {
        return Some(if l.close.is_some_and(|c| c.contains(x, y)) {
            Hit::Close
        } else {
            Hit::Chrome
        });
    }
    if l.tabs.contains(x, y) {
        return Some(
            (0..3)
                .find(|&i| tab_box(l.tabs, i).contains(x, y))
                .map_or(Hit::Chrome, |i| Hit::Tab(Tab::ALL[i])),
        );
    }
    if l.buttons.contains(x, y) {
        return Some(
            (0..BUTTON_LIST.len())
                .find(|&i| button_box(l.buttons, i).contains(x, y))
                .map_or(Hit::Chrome, |i| Hit::Button(BUTTON_LIST[i].0)),
        );
    }
    let row = ((y - l.list.y + scroll) / ROW).floor();
    if row >= 0.0 && (row as usize) < rows {
        Some(Hit::Row(row as usize))
    } else {
        Some(Hit::Chrome)
    }
}

/// `text` cut to fit `width`, with an ellipsis where it is cut.
fn fit(label: &mut Label, size: f32, bold: bool, text: &str, width: f32) -> String {
    let font = quill_layout::fonts::UI_FONT;
    if label.width(font, size, bold, text) <= width {
        return text.to_string();
    }
    let mut s: String = text.to_string();
    while !s.is_empty() && label.width(font, size, bold, &format!("{s}…")) > width {
        s.pop();
    }
    format!("{s}…")
}

/// What the document has at the insertion point: the paragraph's style and
/// the character style, drawn as the current rows.
#[derive(Clone, Copy, Debug)]
pub struct Current {
    pub paragraph: StyleId,
    pub character: u16,
}

pub fn is_current(e: Entry, cur: Current) -> bool {
    match e {
        Entry::NoParagraph | Entry::ClearParagraph | Entry::ClearCharacter => false,
        Entry::Paragraph(id) => id == cur.paragraph,
        Entry::Character(id) => id == cur.character,
    }
}

pub fn draw(c: &mut Canvas, label: &mut Label, p: &Palette, b: Rect, doc: &Document, cur: Current) {
    let font = quill_layout::fonts::UI_FONT;
    let os = p.window.is_some();
    let l = layout(b, os);
    let clip = c.set_clip(b);
    c.fill(b, color::WHITE);

    // A docked palette has a thin title bar of its own, with the close
    // button where the system puts it, a little smaller than its own.
    if !os {
        c.fill(l.title, 0xE4E4E4);
        c.hline(b.x, b.right(), l.title.bottom() - c.hair(), 0xC4C4C4);
        if let Some(x) = l.close {
            let hot = p.hover == Some(Hit::Close);
            if CLOSE_AT_LEFT {
                let (cx, cy) = (x.x + x.w / 2.0, x.y + x.h / 2.0);
                c.circle(cx, cy, 5.5, 0xE0443E);
                c.circle(cx, cy, 4.8, 0xFF5F57);
                if hot {
                    label.draw(c, font, 10.0, true, cx - 3.2, cy + 3.6, "×", 0x4D0000);
                }
            } else {
                if hot {
                    c.fill(x, 0xE81123);
                }
                let ink = if hot { color::WHITE } else { 0x555555 };
                label.draw(c, font, 13.0, false, x.x + 5.0, x.y + 13.0, "×", ink);
            }
        }
        let title = "Style Sheets";
        let w = label.width(font, 11.0, true, title);
        label.draw(
            c,
            font,
            11.0,
            true,
            b.x + (b.w - w) / 2.0,
            l.title.y + 14.0,
            title,
            color::BLACK,
        );
    }

    // Tabs, with the dock button at the end of the row in a window of its
    // own.
    c.fill(Rect::new(b.x, l.tabs.y, b.w, TABS), 0xF3F3F3);
    c.hline(b.x, b.right(), l.tabs.bottom() - c.hair(), 0xD0D0D0);
    {
        let d = l.dock;
        if p.hover == Some(Hit::Dock) {
            c.fill(d, 0xE0E0E0);
        }
        // The arrow points at the edge it docks to, or away from it.
        let arrow = if os { "⇥" } else { "⇱" };
        label.draw(c, font, 12.0, false, d.x + 5.0, d.y + 13.0, arrow, 0x555555);
    }
    let narrow = tab_box(l.tabs, 0).w < 64.0;
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let r = tab_box(l.tabs, i);
        let on = tab == p.tab;
        if on {
            c.fill(r, color::WHITE);
            c.frame(r, 0xC4C4C4);
            c.fill(
                Rect::new(
                    r.x + c.hair(),
                    r.bottom() - c.hair(),
                    r.w - 2.0 * c.hair(),
                    2.0,
                ),
                color::WHITE,
            );
        } else if p.hover == Some(Hit::Tab(tab)) {
            c.fill(r, 0xE8E8E8);
        }
        let text = if narrow { tab.short() } else { tab.name() };
        let tw = label.width(font, 12.0, on, text);
        label.draw(
            c,
            font,
            12.0,
            on,
            r.x + (r.w - tw) / 2.0,
            r.y + 15.0,
            text,
            if on { color::BLACK } else { 0x444444 },
        );
    }

    // Buttons.
    c.fill(l.buttons, 0xFAFAFA);
    c.hline(b.x, b.right(), l.buttons.bottom() - c.hair(), 0xD0D0D0);
    for (i, (button, text)) in BUTTON_LIST.into_iter().enumerate() {
        let r = button_box(l.buttons, i);
        let enabled = match button {
            Button::New => true,
            Button::Edit | Button::Copy => can_edit(p.chosen),
            Button::Delete => can_delete(doc, p.chosen),
        };
        c.fill(
            r,
            if enabled && p.hover == Some(Hit::Button(button)) {
                color::HOVER
            } else {
                color::WHITE
            },
        );
        c.frame(r, 0xCCCCCC);
        let tw = label.width(font, 11.0, false, text);
        label.draw(
            c,
            font,
            11.0,
            false,
            r.x + (r.w - tw) / 2.0,
            r.y + 14.0,
            text,
            if enabled {
                color::BLACK
            } else {
                color::GRAY_DARK
            },
        );
    }

    // The list.
    let rows = entries(doc, p.tab);
    let list_clip = c.set_clip(l.list);
    for (i, e) in rows.iter().enumerate() {
        let y = l.list.y + i as f32 * ROW - p.scroll;
        if y + ROW < l.list.y || y > l.list.bottom() {
            continue;
        }
        let r = Rect::new(b.x, y, b.w, ROW);
        let current = is_current(*e, cur);
        if current {
            c.fill(r, color::HIGHLIGHT);
        } else if p.hover == Some(Hit::Row(i)) {
            c.fill(r, 0xEDEDED);
        }
        let (mark, none) = match e {
            Entry::NoParagraph | Entry::ClearParagraph => ("¶", true),
            Entry::ClearCharacter => ("A", true),
            Entry::Paragraph(_) => ("¶", false),
            Entry::Character(id) => ("A", *id == 0),
        };
        label.draw(c, font, 12.0, false, b.x + 8.0, y + 14.0, mark, 0x666666);
        let text = fit(label, 12.0, false, &name(doc, *e), b.w - 36.0);
        if none {
            label.draw_italic(
                c,
                font,
                12.0,
                false,
                b.x + 26.0,
                y + 14.0,
                &text,
                color::BLACK,
            );
        } else {
            label.draw(
                c,
                font,
                12.0,
                false,
                b.x + 26.0,
                y + 14.0,
                &text,
                color::BLACK,
            );
        }
    }
    c.restore_clip(list_clip);

    // Edges.
    c.frame(b, 0xB8B8B8);
    c.restore_clip(clip);
}

/// A point inside `h` of a palette in `b`, not scrolled, for tests.
#[cfg(test)]
pub fn point_of(b: Rect, os: bool, h: Hit) -> (f32, f32) {
    let l = layout(b, os);
    let r = match h {
        Hit::Close => l.close.unwrap(),
        Hit::Dock => l.dock,
        Hit::Tab(t) => tab_box(l.tabs, t.index() as usize),
        Hit::Button(k) => {
            let i = BUTTON_LIST.iter().position(|(b, _)| *b == k).unwrap();
            button_box(l.buttons, i)
        }
        Hit::Row(i) => Rect::new(l.list.x, l.list.y + i as f32 * ROW, l.list.w, ROW),
        Hit::Chrome => l.list,
    };
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::character_styles::CharacterFormat;
    use quill_core::props::CharProps;

    fn doc() -> Document {
        let mut doc = Document::new();
        let bold = CharProps {
            bold: true,
            ..CharProps::default()
        };
        doc.styles
            .add_character("Strong", CharacterFormat::new(bold, 1 << 2));
        doc.styles
            .add_character("Accent", CharacterFormat::new(bold, 1 << 2));
        doc
    }

    #[test]
    fn tabs_filter_one_list() {
        let doc = doc();
        let all = entries(&doc, Tab::All);
        let paragraph = entries(&doc, Tab::Paragraph);
        let character = entries(&doc, Tab::Character);
        assert!(paragraph.iter().all(|e| matches!(
            e,
            Entry::Paragraph(_) | Entry::NoParagraph | Entry::ClearParagraph
        )));
        assert!(
            character
                .iter()
                .all(|e| matches!(e, Entry::Character(_) | Entry::ClearCharacter))
        );
        assert_eq!(all.len(), paragraph.len() + character.len());
        // All is the paragraph styles followed by the character styles.
        assert_eq!(all[..paragraph.len()], paragraph[..]);
        assert_eq!(all[paragraph.len()..], character[..]);
        // Normal and No Style lead their groups.
        assert_eq!(paragraph[0], Entry::NoParagraph);
        assert_eq!(paragraph[1], Entry::ClearParagraph);
        assert_eq!(paragraph[2], Entry::Paragraph(0));
        assert_eq!(character[0], Entry::Character(0));
        assert_eq!(character[1], Entry::ClearCharacter);
    }

    #[test]
    fn character_styles_are_sorted_by_name() {
        let doc = doc();
        let names: Vec<String> = entries(&doc, Tab::Character)
            .into_iter()
            .map(|e| name(&doc, e))
            .collect();
        assert_eq!(names, ["No Style", "Clear Formatting", "Accent", "Strong"]);
    }

    #[test]
    fn no_style_and_automatic_styles_cannot_be_deleted() {
        let doc = doc();
        assert!(!can_delete(&doc, Some(Entry::Paragraph(0))));
        assert!(!can_delete(&doc, Some(Entry::Paragraph(1))));
        assert!(!can_delete(&doc, Some(Entry::Character(0))));
        assert!(can_delete(&doc, Some(Entry::Character(1))));
        assert!(!can_edit(Some(Entry::Character(0))));
        assert!(!can_edit(None));
    }

    #[test]
    fn clicks_land_on_the_part_they_are_over() {
        let b = Rect::new(100.0, 50.0, DOCK_WIDTH, 300.0);
        assert_eq!(hit(b, false, 0.0, 5, 99.0, 60.0), None);
        // The docked palette has a title bar with its close and dock boxes.
        let close = point_of(b, false, Hit::Close);
        assert_eq!(hit(b, false, 0.0, 5, close.0, close.1), Some(Hit::Close));
        let dock = point_of(b, false, Hit::Dock);
        assert_eq!(hit(b, false, 0.0, 5, dock.0, dock.1), Some(Hit::Dock));
        assert_eq!(hit(b, false, 0.0, 5, 200.0, 55.0), Some(Hit::Chrome));
        let tabs_y = b.y + TITLE + 10.0;
        assert_eq!(
            hit(b, false, 0.0, 5, 110.0, tabs_y),
            Some(Hit::Tab(Tab::All))
        );
        assert_eq!(
            hit(b, false, 0.0, 5, b.right() - 10.0, tabs_y),
            Some(Hit::Tab(Tab::Character))
        );
        let buttons_y = b.y + TITLE + TABS + 10.0;
        assert_eq!(
            hit(b, false, 0.0, 5, 110.0, buttons_y),
            Some(Hit::Button(Button::New))
        );
        let list_y = b.y + TITLE + TABS + BUTTONS;
        assert_eq!(
            hit(b, false, 0.0, 5, 150.0, list_y + 1.0),
            Some(Hit::Row(0))
        );
        assert_eq!(
            hit(b, false, 0.0, 5, 150.0, list_y + 2.5 * ROW),
            Some(Hit::Row(2))
        );
        // Scrolled by a row and a half, the same place is further down.
        assert_eq!(
            hit(b, false, 1.5 * ROW, 5, 150.0, list_y + 1.0),
            Some(Hit::Row(1))
        );
        // Below the last row is the palette's own empty space.
        assert_eq!(
            hit(b, false, 0.0, 5, 150.0, list_y + 5.5 * ROW),
            Some(Hit::Chrome)
        );
    }

    #[test]
    fn in_a_window_of_its_own_the_tabs_start_at_the_top_and_share_the_row_with_dock() {
        let b = Rect::new(0.0, 0.0, 200.0, 300.0);
        let tab = point_of(b, true, Hit::Tab(Tab::All));
        assert!(tab.1 < TABS);
        assert_eq!(hit(b, true, 0.0, 5, tab.0, tab.1), Some(Hit::Tab(Tab::All)));
        let last = point_of(b, true, Hit::Tab(Tab::Character));
        assert_eq!(
            hit(b, true, 0.0, 5, last.0, last.1),
            Some(Hit::Tab(Tab::Character))
        );
        let dock = point_of(b, true, Hit::Dock);
        assert!(dock.0 > last.0);
        assert_eq!(hit(b, true, 0.0, 5, dock.0, dock.1), Some(Hit::Dock));
    }

    #[test]
    fn the_list_scrolls_only_as_far_as_its_rows() {
        let b = Rect::new(0.0, 0.0, DOCK_WIDTH, 300.0);
        let list = 300.0 - (TITLE + TABS + BUTTONS);
        assert_eq!(max_scroll(b, false, 3), 0.0);
        assert_eq!(max_scroll(b, false, 40), 40.0 * ROW - list);
        // Without a title bar the list has that much more room.
        assert_eq!(max_scroll(b, true, 40), 40.0 * ROW - list - TITLE);
    }

    #[test]
    fn tabs_round_trip_through_their_saved_number() {
        for tab in Tab::ALL {
            assert_eq!(Tab::from_index(tab.index()), tab);
        }
        assert_eq!(Tab::from_index(9), Tab::All);
    }
}
