//! The Style Sheets palette: one list of the document's paragraph and
//! character styles, with tabs to show All, only Paragraph or only
//! Character styles, and a row of buttons to make, change, copy and delete
//! them. It docks against the right side of the document window, or comes
//! out into a small window of its own that can go anywhere on the screen.

use crate::canvas::{Canvas, Label, Rect, color, round_rect};
use quill_core::Document;
use quill_core::props::StyleId;
use std::time::Instant;

/// Heights and widths in interface units.
const TITLE: f32 = 24.0;
const TABS: f32 = 20.0;
const BUTTONS: f32 = 22.0;
const ROW: f32 = 20.0;
/// The docked width, and the size of the window it comes out into.
pub const DOCK_WIDTH: f32 = 220.0;
pub const DOCK_MIN_WIDTH: f32 = 140.0;
pub const WINDOW_SIZE: (f32, f32) = (210.0, 310.0);
/// The smallest the palette's own window goes.
pub const WINDOW_MIN: (f32, f32) = (200.0, 160.0);
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

const FOOTER: [(Hit, &str); 3] = [
    (Hit::Button(Button::New), "+"),
    (Hit::Button(Button::Delete), "−"),
    (Hit::More, "⋯"),
];
const ACTIONS: [(Button, &str); 3] = [
    (Button::Edit, "Edit Style…"),
    (Button::Copy, "Duplicate"),
    (Button::Delete, "Delete"),
];
const ACTION_WIDTH: f32 = 136.0;

/// What a click on the palette hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    More,
    Close,
    Title,
    /// Dock or undock.
    Dock,
    Tab(Tab),
    ToggleScope,
    Button(Button),
    Row(usize),
    /// Palette furniture that does nothing.
    Chrome,
}

pub struct Palette {
    pub tab: Tab,
    pub show_all: bool,
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
    pub dock_width: f32,
    pub resize_grab: Option<f32>,
    pub title_press: Option<(f32, f32)>,
    pub actions_at: Option<(f32, f32)>,
    pub last_row_click: Option<(Entry, Instant)>,
}

impl Palette {
    pub fn new(tab: Tab, docked: bool) -> Palette {
        Palette {
            tab,
            show_all: false,
            docked,
            window: None,
            scroll: 0.0,
            chosen: None,
            hover: None,
            dock_width: DOCK_WIDTH,
            resize_grab: None,
            title_press: None,
            actions_at: None,
            last_row_click: None,
        }
    }

    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.actions_at = None;
        self.last_row_click = None;
        self.chosen = None;
        self.scroll = 0.0;
    }
}

/// The rows the tab shows. Each tab starts with its fixed rows: for
/// paragraph styles "No Style", Clear Formatting and Normal, for character
/// styles "No Style" and Clear Formatting. The named styles follow, sorted
/// by name. The All tab has only the paragraph fixed rows, and the two kinds
/// of style share one list after them, so they are sorted together.
pub fn entries(doc: &Document, tab: Tab, show_all: bool) -> Vec<Entry> {
    let mut rows = Vec::new();
    let mut named: Vec<(String, Entry)> = Vec::new();
    if tab != Tab::Character {
        rows.push(Entry::NoParagraph);
        rows.push(Entry::ClearParagraph);
        rows.push(Entry::Paragraph(0));
        named.extend(
            visible_paragraph_styles(doc, show_all)
                .into_iter()
                .map(|id| {
                    let e = Entry::Paragraph(id);
                    (name(doc, e).to_lowercase(), e)
                }),
        );
    }
    if tab != Tab::Paragraph {
        if tab == Tab::Character {
            rows.push(Entry::Character(0));
            rows.push(Entry::ClearCharacter);
        }
        named.extend((1..=doc.styles.characters.len() as u16).map(|id| {
            let e = Entry::Character(id);
            (name(doc, e).to_lowercase(), e)
        }));
    }
    // A stable sort keeps a paragraph style ahead of a character style
    // with the same name.
    named.sort_by(|a, b| a.0.cmp(&b.0));
    rows.extend(named.into_iter().map(|(_, e)| e));
    rows
}

fn visible_paragraph_styles(doc: &Document, show_all: bool) -> Vec<StyleId> {
    if show_all {
        (1..doc.styles.styles.len() as StyleId).collect()
    } else {
        crate::document_styles::paragraph_styles(doc, None)
            .into_iter()
            .filter(|&id| id != 0)
            .collect()
    }
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
        Some(Entry::Character(id)) => doc.styles.can_delete_character(id),
        Some(Entry::NoParagraph | Entry::ClearParagraph | Entry::ClearCharacter) | None => false,
    }
}

struct Layout {
    title: Rect,
    tabs: Rect,
    buttons: Rect,
    scope: Rect,
    list: Rect,
    dock: Rect,
}

fn layout(b: Rect, floating: bool) -> Layout {
    let title = Rect::new(
        b.x,
        b.y,
        b.w,
        if floating && cfg!(target_os = "macos") {
            0.0
        } else {
            TITLE
        },
    );
    let dock = Rect::new(b.right() - 27.0, title.y + 2.0, 21.0, TITLE - 4.0);
    let tabs = Rect::new(b.x + 6.0, title.bottom(), b.w - 12.0, TABS);
    let buttons = Rect::new(b.x, (b.bottom() - BUTTONS).max(b.y), b.w, BUTTONS);
    let scope = Rect::new(b.x, (buttons.y - BUTTONS).max(b.y), b.w, BUTTONS);
    let top = tabs.bottom() + 5.0;
    let list = Rect::new(b.x, top, b.w, (scope.y - top).max(0.0));
    Layout {
        title,
        tabs,
        buttons,
        scope,
        list,
        dock,
    }
}

fn tab_box(tabs: Rect, i: usize) -> Rect {
    let widths = [0.21, 0.395, 0.395];
    let left: f32 = widths[..i].iter().sum();
    Rect::new(tabs.x + tabs.w * left, tabs.y, tabs.w * widths[i], tabs.h)
}

fn button_box(buttons: Rect, i: usize) -> Rect {
    let x = if i == 2 {
        buttons.right() - 29.0
    } else {
        buttons.x + 5.0 + i as f32 * 24.0
    };
    Rect::new(x, buttons.y + 1.0, 24.0, BUTTONS - 2.0)
}

fn action_bounds(b: Rect, (x, y): (f32, f32)) -> Rect {
    let h = ACTIONS.len() as f32 * ROW;
    Rect::new(
        x.clamp(b.x, (b.right() - ACTION_WIDTH).max(b.x)),
        (y - h).max(b.y),
        ACTION_WIDTH,
        h,
    )
}

pub fn action_at(b: Rect, anchor: (f32, f32), x: f32, y: f32) -> Option<Button> {
    let r = action_bounds(b, anchor);
    r.contains(x, y)
        .then(|| ACTIONS[((y - r.y) / ROW) as usize].0)
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
    if l.title.h > 0.0 && l.dock.contains(x, y) {
        return Some(if os { Hit::Dock } else { Hit::Close });
    }
    if l.title.contains(x, y) {
        return Some(if os { Hit::Chrome } else { Hit::Title });
    }
    if l.tabs.contains(x, y) {
        return Some(
            (0..3)
                .find(|&i| tab_box(l.tabs, i).contains(x, y))
                .map_or(Hit::Chrome, |i| Hit::Tab(Tab::ALL[i])),
        );
    }
    if l.scope.contains(x, y) {
        return Some(Hit::ToggleScope);
    }
    if l.buttons.contains(x, y) {
        return Some(
            (0..FOOTER.len())
                .find(|&i| button_box(l.buttons, i).contains(x, y))
                .map_or(Hit::Chrome, |i| FOOTER[i].0),
        );
    }
    if !l.list.contains(x, y) {
        return Some(Hit::Chrome);
    }
    let scroll = scroll.min(max_scroll(b, os, rows));
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

/// Diagonal arrows for moving between docked and floating states.
pub fn draw_docking_icon(c: &mut Canvas, label: &mut Label, r: Rect, dock: bool) {
    const IN: &str = "line k/1.5 2 2 6 6\nline k/1.5 3 6 6 6 6 3\nline k/1.5 13 13 9 9\nline k/1.5 12 9 9 9 9 12";
    const OUT: &str = "line k/1.5 6 6 2 2\nline k/1.5 2 5 2 2 5 2\nline k/1.5 9 9 13 13\nline k/1.5 10 13 13 13 13 10";
    let picture = crate::icon::cached(if dock { IN } else { OUT }, 16, 16, c.scale);
    crate::icon::paint(
        c,
        label,
        &picture,
        r.x + (r.w - 16.0) / 2.0,
        r.y + (r.h - 16.0) / 2.0,
        255,
        0xF5F5F7,
    );
}

pub fn draw(
    c: &mut Canvas,
    label: &mut Label,
    p: &Palette,
    b: Rect,
    doc: &Document,
    cur: Current,
    rows: &[Entry],
) {
    let font = quill_layout::fonts::UI_FONT;
    let os = p.window.is_some();
    let l = layout(b, os);
    let clip = c.set_clip(b);
    c.fill(b, 0xF5F5F7);
    if !os {
        c.vline(b.x, b.y, b.bottom(), 0xD5D5DA);
        label.draw(
            c,
            font,
            10.0,
            true,
            b.x + 8.0,
            l.title.y + 15.0,
            "STYLE SHEETS",
            0x55555C,
        );
    }
    let d = l.dock;
    let control = if os { Hit::Dock } else { Hit::Close };
    if l.title.h > 0.0 && p.hover == Some(control) {
        round_rect(c, d, 4.0, 0xE0E0E5);
    }
    if os && l.title.h > 0.0 {
        draw_docking_icon(c, label, d, true);
    } else if !os {
        label.draw(c, font, 13.0, false, d.x + 6.5, d.y + 14.0, "×", 0x505058);
    }

    round_rect(c, l.tabs, 5.0, 0xE6E6EB);
    let full_labels_fit = [Tab::Paragraph, Tab::Character].into_iter().all(|tab| {
        label.width(font, 11.0, true, tab.name()) + 8.0 <= tab_box(l.tabs, tab.index() as usize).w
    });
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let r = tab_box(l.tabs, i);
        let on = tab == p.tab;
        if on || p.hover == Some(Hit::Tab(tab)) {
            round_rect(
                c,
                Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0),
                4.0,
                if on { color::WHITE } else { 0xEEEEF0 },
            );
        }
        let text = if full_labels_fit {
            tab.name()
        } else {
            match tab {
                Tab::All => "All",
                Tab::Paragraph => "¶",
                Tab::Character => "Abc",
            }
        };
        let tw = label.width(font, 11.0, on, text);
        label.draw(
            c,
            font,
            11.0,
            on,
            r.x + (r.w - tw) / 2.0,
            r.y + 14.0,
            text,
            0x36363D,
        );
    }

    c.hline(b.x, b.right(), l.buttons.y, 0xD5D5DA);
    let target = p.chosen.or(Some(Entry::Paragraph(cur.paragraph)));
    for (i, (hit, text)) in FOOTER.into_iter().enumerate() {
        let r = button_box(l.buttons, i);
        let enabled = hit != Hit::Button(Button::Delete) || can_delete(doc, target);
        if enabled && p.hover == Some(hit) {
            round_rect(c, r, 4.0, 0xE0E0E5);
        }
        let tw = label.width(font, 14.0, false, text);
        label.draw(
            c,
            font,
            14.0,
            false,
            r.x + (r.w - tw) / 2.0,
            r.y + 15.0,
            text,
            if enabled { 0x33333A } else { 0x88888E },
        );
    }

    c.hline(b.x, b.right(), l.scope.y, 0xD5D5DA);
    if p.hover == Some(Hit::ToggleScope) {
        round_rect(
            c,
            Rect::new(
                l.scope.x + 4.0,
                l.scope.y + 2.0,
                l.scope.w - 8.0,
                l.scope.h - 4.0,
            ),
            4.0,
            0xE0E0E5,
        );
    }
    // Label at the left, on/off switch against the right edge.
    label.draw(
        c,
        font,
        11.0,
        false,
        l.scope.x + 12.0,
        l.scope.y + 15.0,
        "Show All Styles",
        0x36363D,
    );
    crate::dialog::draw_switch(
        c,
        Rect::new(
            l.scope.right() - 12.0 - crate::dialog::SWITCH_W,
            l.scope.y + (l.scope.h - crate::dialog::SWITCH_H) / 2.0,
            crate::dialog::SWITCH_W,
            crate::dialog::SWITCH_H,
        ),
        p.show_all,
    );

    // The list.
    let list_clip = c.set_clip(l.list);
    for (i, e) in rows.iter().enumerate() {
        let y = l.list.y + i as f32 * ROW - p.scroll;
        if y + ROW < l.list.y || y > l.list.bottom() {
            continue;
        }
        let r = Rect::new(b.x + 6.0, y + 1.0, b.w - 12.0, ROW - 2.0);
        let current = is_current(*e, cur);
        if current {
            round_rect(c, r, 5.0, 0x0066CC);
        } else if p.hover == Some(Hit::Row(i)) {
            round_rect(c, r, 5.0, 0xE8E8EC);
        }
        let (mark, none) = match e {
            Entry::NoParagraph | Entry::ClearParagraph => ("¶", true),
            Entry::ClearCharacter => ("A", true),
            Entry::Paragraph(_) => ("¶", false),
            Entry::Character(id) => ("A", *id == 0),
        };
        let ink = if current && !c.dark {
            color::WHITE
        } else {
            color::BLACK
        };
        label.draw(
            c,
            font,
            11.5,
            false,
            b.x + 12.0,
            y + 14.0,
            mark,
            if current { 0xCDE4FF } else { 0x777780 },
        );
        let text = fit(label, 11.5, false, &name(doc, *e), b.w - 36.0);
        if none {
            label.draw_italic(c, font, 11.5, false, b.x + 26.0, y + 14.0, &text, ink);
        } else {
            label.draw(c, font, 11.5, false, b.x + 26.0, y + 14.0, &text, ink);
        }
    }
    c.restore_clip(list_clip);

    if let Some(anchor) = p.actions_at {
        let menu = action_bounds(b, anchor);
        round_rect(c, menu, 5.0, color::WHITE);
        c.frame(menu, 0xC6C6CC);
        for (i, (button, text)) in ACTIONS.into_iter().enumerate() {
            let enabled = if button == Button::Delete {
                can_delete(doc, target)
            } else {
                can_edit(target)
            };
            let y = menu.y + i as f32 * ROW;
            if enabled && p.hover == Some(Hit::Button(button)) {
                round_rect(
                    c,
                    Rect::new(menu.x + 3.0, y + 1.0, menu.w - 6.0, ROW - 2.0),
                    4.0,
                    0xE6E6EB,
                );
            }
            label.draw(
                c,
                font,
                11.5,
                false,
                menu.x + 9.0,
                y + 14.0,
                text,
                if enabled { color::BLACK } else { 0x88888E },
            );
        }
    }
    c.restore_clip(clip);
}

#[cfg(test)]
pub fn action_point(b: Rect, anchor: (f32, f32), button: Button) -> (f32, f32) {
    let bounds = action_bounds(b, anchor);
    let i = ACTIONS
        .iter()
        .position(|(value, _)| *value == button)
        .unwrap();
    (bounds.x + bounds.w / 2.0, bounds.y + (i as f32 + 0.5) * ROW)
}

/// A point inside `h` of a palette in `b`, not scrolled, for tests.
#[cfg(test)]
pub fn point_of(b: Rect, os: bool, h: Hit) -> (f32, f32) {
    let l = layout(b, os);
    let r = match h {
        Hit::ToggleScope => l.scope,
        Hit::More => button_box(l.buttons, 2),
        Hit::Dock | Hit::Close => l.dock,
        Hit::Title => l.title,
        Hit::Tab(t) => tab_box(l.tabs, t.index() as usize),
        Hit::Button(k) => {
            let i = FOOTER
                .iter()
                .position(|(hit, _)| *hit == Hit::Button(k))
                .unwrap();
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
    use quill_core::StoryId;
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
        let all = entries(&doc, Tab::All, false);
        let paragraph = entries(&doc, Tab::Paragraph, false);
        let character = entries(&doc, Tab::Character, false);
        assert!(paragraph.iter().all(|e| matches!(
            e,
            Entry::Paragraph(_) | Entry::NoParagraph | Entry::ClearParagraph
        )));
        assert!(
            character
                .iter()
                .all(|e| matches!(e, Entry::Character(_) | Entry::ClearCharacter))
        );
        // All has one "No Style" and one Clear Formatting, the paragraph ones.
        assert_eq!(all.len(), paragraph.len() + character.len() - 2);
        assert!(!all.contains(&Entry::Character(0)));
        assert!(!all.contains(&Entry::ClearCharacter));
        // All sorts both kinds of style together after the fixed rows.
        let names: Vec<String> = all.iter().map(|&e| name(&doc, e)).collect();
        let sorted = &names[3..];
        let mut want = sorted.to_vec();
        want.sort_by_key(|n| n.to_lowercase());
        assert_eq!(sorted, &want[..]);
        assert!(sorted.iter().any(|n| n == "Accent"));
        assert!(sorted.iter().any(|n| n == "Heading 1"));
        let accent = sorted.iter().position(|n| n == "Accent").unwrap();
        let heading = sorted.iter().position(|n| n == "Heading 1").unwrap();
        assert!(accent < heading);
        // Normal and No Style lead their groups.
        assert_eq!(paragraph[0], Entry::NoParagraph);
        assert_eq!(paragraph[1], Entry::ClearParagraph);
        assert_eq!(paragraph[2], Entry::Paragraph(0));
        for style in ["Title", "Subtitle"] {
            let entry = Entry::Paragraph(doc.styles.find(style).unwrap());
            assert!(paragraph.contains(&entry));
            assert!(all.contains(&entry));
        }
        assert_eq!(character[0], Entry::Character(0));
        assert_eq!(character[1], Entry::ClearCharacter);
    }

    fn shows(doc: &Document, style: &str) -> bool {
        let id = doc.styles.find(style).unwrap();
        entries(doc, Tab::Paragraph, false).contains(&Entry::Paragraph(id))
    }

    #[test]
    fn later_headings_follow_the_highest_heading_used() {
        let mut doc = quill_core::text::from_plain_text("Heading");
        for level in 1..=9 {
            assert_eq!(shows(&doc, &format!("heading {level}")), level <= 3);
        }
        for level in 2..=9 {
            let id = doc.styles.find(&format!("heading {level}")).unwrap();
            doc.apply_style(StoryId::Main, 0, 0, id);
            for visible in 1..=9 {
                assert_eq!(
                    shows(&doc, &format!("heading {visible}")),
                    visible <= level + 2
                );
            }
        }
        doc.apply_style(StoryId::Main, 0, 0, 0);
        assert!(!shows(&doc, "heading 4"));
    }

    #[test]
    fn header_footer_and_footnote_styles_follow_document_content() {
        use quill_core::{Position, Selection};

        let mut doc = Document::new();
        for style in ["header", "footer", "footnote reference", "footnote text"] {
            assert!(!shows(&doc, style));
        }
        let props = doc.styles.get(0).chp;
        doc.insert_plain(StoryId::FirstHeader, Position::new(0, 0), "Title", props);
        assert!(shows(&doc, "header"));
        assert!(!shows(&doc, "footer"));
        doc.insert_plain(StoryId::EvenFooter, Position::new(0, 0), "Page", props);
        assert!(shows(&doc, "footer"));

        doc.begin("Insert Footnote", Selection::default());
        doc.insert_note(Position::new(0, 0), props, None);
        doc.commit(Selection::default());
        assert!(shows(&doc, "footnote reference"));
        assert!(shows(&doc, "footnote text"));
        doc.undo();
        assert!(!shows(&doc, "footnote reference"));
        assert!(!shows(&doc, "footnote text"));
    }

    #[test]
    fn only_toc_levels_in_the_generated_contents_are_listed() {
        use quill_core::{Position, Selection};

        let mut doc = quill_core::text::from_plain_text("Chapter\nTopic");
        for (para, style) in [(0, "heading 1"), (1, "heading 3")] {
            let id = doc.styles.find(style).unwrap();
            doc.apply_style(StoryId::Main, para, para, id);
        }
        assert!(!shows(&doc, "toc 1"));
        let entries: Vec<_> = doc
            .toc_entries(&Default::default())
            .into_iter()
            .map(|entry| (entry, String::new()))
            .collect();
        let paras = doc.toc_paragraphs(&entries, false);
        doc.begin("Insert Table of Contents", Selection::default());
        doc.place_toc(paras, Position::new(0, 0));
        doc.commit(Selection::default());

        for level in 1..=9 {
            assert_eq!(shows(&doc, &format!("toc {level}")), matches!(level, 1 | 3));
        }
        doc.undo();
        assert!(!shows(&doc, "toc 1"));
        assert!(!shows(&doc, "toc 3"));
    }

    #[test]
    fn index_styles_follow_usage_and_custom_styles_are_always_visible() {
        use quill_core::{Paragraph, Selection};

        let mut doc = quill_core::text::from_plain_text("Body");
        let index = doc.styles.find("index 1").unwrap();
        let custom = doc.add_style(
            "My unused style",
            0,
            None,
            Default::default(),
            Default::default(),
        );
        let normal = entries(&doc, Tab::Paragraph, false);
        assert!(normal.contains(&Entry::Paragraph(custom)));
        assert!(!normal.contains(&Entry::Paragraph(index)));
        assert!(entries(&doc, Tab::Paragraph, true).contains(&Entry::Paragraph(index)));

        let style = doc.styles.get(index);
        let entry = Paragraph::with_text("Term", style.chp, style.pap.clone());
        doc.begin("Insert Index", Selection::default());
        doc.place_index(vec![entry]);
        doc.commit(Selection::default());
        assert!(shows(&doc, "index 1"));
        assert!(!shows(&doc, "index 2"));
        doc.undo();
        assert!(!shows(&doc, "index 1"));
    }

    #[test]
    fn character_styles_are_sorted_by_name() {
        let doc = doc();
        let names: Vec<String> = entries(&doc, Tab::Character, false)
            .into_iter()
            .map(|e| name(&doc, e))
            .collect();
        assert_eq!(
            names,
            ["No Style", "Clear Formatting", "Accent", "Normal", "Strong"]
        );
    }

    #[test]
    fn no_style_and_automatic_styles_cannot_be_deleted() {
        let doc = doc();
        assert!(!can_delete(&doc, Some(Entry::Paragraph(0))));
        assert!(!can_delete(&doc, Some(Entry::Paragraph(1))));
        assert!(!can_delete(&doc, Some(Entry::Character(0))));
        assert!(!can_delete(&doc, Some(Entry::Character(1))));
        assert!(can_delete(&doc, Some(Entry::Character(2))));
        assert!(!can_edit(Some(Entry::Character(0))));
        assert!(!can_edit(None));
    }

    #[test]
    fn clicks_land_on_the_part_they_are_over() {
        let b = Rect::new(100.0, 50.0, DOCK_WIDTH, 300.0);
        assert_eq!(hit(b, false, 0.0, 5, 99.0, 60.0), None);
        let close = point_of(b, false, Hit::Close);
        assert_eq!(hit(b, false, 0.0, 5, close.0, close.1), Some(Hit::Close));
        assert_eq!(hit(b, false, 0.0, 5, 200.0, 55.0), Some(Hit::Title));
        let tabs_y = b.y + TITLE + 10.0;
        assert_eq!(
            hit(b, false, 0.0, 5, 110.0, tabs_y),
            Some(Hit::Tab(Tab::All))
        );
        assert_eq!(
            hit(b, false, 0.0, 5, b.right() - 10.0, tabs_y),
            Some(Hit::Tab(Tab::Character))
        );
        let buttons_y = b.bottom() - BUTTONS + 10.0;
        assert_eq!(
            hit(b, false, 0.0, 5, 110.0, buttons_y),
            Some(Hit::Button(Button::New))
        );
        let list_y = b.y + TITLE + TABS + 5.0;
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
            hit(b, false, 1.5 * ROW, 40, 150.0, list_y + 1.0),
            Some(Hit::Row(1))
        );
        // A list that shrank back to five rows fits without scrolling.
        assert_eq!(
            hit(b, false, 1.5 * ROW, 5, 150.0, list_y + 1.0),
            Some(Hit::Row(0))
        );
        // Below the last row is the palette's own empty space.
        assert_eq!(
            hit(b, false, 0.0, 5, 150.0, list_y + 5.5 * ROW),
            Some(Hit::Chrome)
        );
    }

    #[test]
    fn floating_filters_have_their_own_row_below_dock() {
        let b = Rect::new(0.0, 0.0, 200.0, 300.0);
        let tab = point_of(b, true, Hit::Tab(Tab::All));
        assert!(tab.1 >= layout(b, true).title.h);
        assert_eq!(hit(b, true, 0.0, 5, tab.0, tab.1), Some(Hit::Tab(Tab::All)));
        let last = point_of(b, true, Hit::Tab(Tab::Character));
        assert_eq!(
            hit(b, true, 0.0, 5, last.0, last.1),
            Some(Hit::Tab(Tab::Character))
        );
        if !cfg!(target_os = "macos") {
            let dock = point_of(b, true, Hit::Dock);
            assert!(dock.1 < last.1);
            assert_eq!(hit(b, true, 0.0, 5, dock.0, dock.1), Some(Hit::Dock));
        }
    }

    #[test]
    fn the_list_scrolls_only_as_far_as_its_rows() {
        let b = Rect::new(0.0, 0.0, DOCK_WIDTH, 300.0);
        let list = 300.0 - (TITLE + TABS + 2.0 * BUTTONS + 5.0);
        assert_eq!(max_scroll(b, false, 3), 0.0);
        assert_eq!(max_scroll(b, false, 40), 40.0 * ROW - list);
        assert_eq!(max_scroll(b, true, 40), 40.0 * ROW - layout(b, true).list.h);
    }

    #[test]
    fn tabs_round_trip_through_their_saved_number() {
        for tab in Tab::ALL {
            assert_eq!(Tab::from_index(tab.index()), tab);
        }
        assert_eq!(Tab::from_index(9), Tab::All);
    }
}
