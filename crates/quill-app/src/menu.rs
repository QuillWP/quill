//! Flat pull-down, popup and context menus for the shared renderer.
//!
//! The menu contents are plain data rebuilt from the application's state
//! whenever they are shown, so enabling, checkmarks and labels like
//! "Undo Typing" are always current.

use crate::canvas::{Canvas, Label, Rect, color, round_rect};

pub const MENU_BAR_HEIGHT: f32 = 20.0;
const ITEM_HEIGHT: f32 = 16.0;
const SEPARATOR_HEIGHT: f32 = 8.0;
fn interface_font() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        quill_layout::fonts::UI_FONT
    }
    #[cfg(windows)]
    {
        crate::win_dialog::menu_font()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        static FONT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        FONT.get_or_init(|| {
            let configured = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "font-name"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().trim_matches('\'').to_string());
            if let Some(font) = configured
                && let Some((family, _)) = font.rsplit_once(' ')
                && !family.is_empty()
            {
                return family.to_string();
            }
            std::process::Command::new("fc-match")
                .args(["--format", "%{family}", "sans-serif"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| s.split(',').next().map(str::to_string))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| quill_layout::fonts::UI_FONT.into())
        })
    }
}
const BOLD: bool = false;
const ACCENT: u32 = 0x0A84FF;
const SIZE: f32 = 12.0;
/// The Appearance accent color for highlighted titles and items.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItemStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub outline: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Item {
    pub label: String,
    /// Shortcut text drawn at the right, like "⌘B".
    pub key: String,
    pub enabled: bool,
    pub checked: bool,
    /// Whether this item represents a persistent selectable state.
    pub checkable: bool,
    pub separator: bool,
    pub style: ItemStyle,
    /// Font to draw the label in (the Font menu shows each font in itself
    /// on later systems; the original did not, so this stays None by default).
    pub font: Option<String>,
    /// A single nested menu, opened to the side of this item.
    pub submenu: Option<Menu>,
    /// Identifies the item to whoever built the menu.
    pub id: u32,
}

impl Item {
    pub fn new(label: impl Into<String>, id: u32) -> Item {
        Item {
            label: label.into(),
            enabled: true,
            id,
            ..Default::default()
        }
    }

    pub fn separator() -> Item {
        Item {
            separator: true,
            ..Default::default()
        }
    }

    pub fn key(mut self, k: &str) -> Item {
        self.key = k.to_string();
        self
    }

    pub fn enabled(mut self, on: bool) -> Item {
        self.enabled = on;
        self
    }

    pub fn checked(mut self, on: bool) -> Item {
        self.checked = on;
        self.checkable = true;
        self
    }

    pub fn style(mut self, s: ItemStyle) -> Item {
        self.style = s;
        self
    }

    /// Draw this item's label in an installed font family.  Font-menu
    /// previews opt in; ordinary menu labels keep the interface face.
    pub fn font(mut self, name: impl Into<String>) -> Item {
        self.font = Some(name.into());
        self
    }

    pub fn submenu(mut self, menu: Menu) -> Item {
        self.submenu = Some(menu);
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct Menu {
    pub title: String,
    pub items: Vec<Item>,
}

/// An open menu: where it is and which item is under the pointer.
#[derive(Clone, Debug, Default)]
pub struct OpenMenu {
    /// Index into the menu list (for the bar) or 0 for a pop-up.
    pub menu: usize,
    pub rect: Rect,
    pub hot: Option<usize>,
    /// Items scrolled off the top when the menu is taller than the window.
    pub scroll: usize,
    /// Rows that fit; less than the item count when scrolling.
    pub visible_rows: usize,
}

fn item_height(it: &Item) -> f32 {
    if it.separator {
        SEPARATOR_HEIGHT
    } else {
        ITEM_HEIGHT
    }
}

/// Lay out the menu bar titles. Returns each title's rectangle.
pub fn title_rects(label: &mut Label, menus: &[Menu], width: f32) -> Vec<Rect> {
    let mut x = 10.0;
    menus
        .iter()
        .map(|m| {
            let w = label.width(interface_font(), SIZE, BOLD, &m.title) + 14.0;
            let r = Rect::new(x, 0.0, w, MENU_BAR_HEIGHT);
            x = (x + w).min(width);
            r
        })
        .collect()
}

pub fn draw_bar(
    c: &mut Canvas,
    label: &mut Label,
    menus: &[Menu],
    width: f32,
    open: Option<usize>,
) {
    let bar = Rect::new(0.0, 0.0, width, MENU_BAR_HEIGHT);
    c.fill(bar, color::GRAY);
    c.hline(0.0, width, MENU_BAR_HEIGHT - 1.0, color::BLACK);
    c.hline(0.0, width, 0.0, color::WHITE);
    let rects = title_rects(label, menus, width);
    for (i, (m, r)) in menus.iter().zip(&rects).enumerate() {
        let on = open == Some(i);
        if on {
            c.fill(Rect::new(r.x, 0.0, r.w, MENU_BAR_HEIGHT - 1.0), ACCENT);
        }
        let fg = if on { color::WHITE } else { color::BLACK };
        label.draw(
            c,
            interface_font(),
            SIZE,
            BOLD,
            r.x + 7.0,
            15.0,
            &m.title,
            fg,
        );
    }
}

/// Size a menu's box and place it below `anchor` (a title, or a pop-up's
/// field), keeping it inside `bounds`.
pub fn place(label: &mut Label, menu: &Menu, anchor: Rect, bounds: Rect, below: bool) -> OpenMenu {
    let mut w: f32 = anchor.w.max(60.0);
    for it in &menu.items {
        let text = label.width(interface_font(), SIZE, BOLD, &it.label);
        let key = if it.key.is_empty() {
            0.0
        } else {
            let key = crate::commands::display_key(&it.key);
            label.width(interface_font(), SIZE, BOLD, &key) + 24.0
        };
        w = w.max(text + key + 36.0);
    }
    let full: f32 = menu.items.iter().map(item_height).sum::<f32>() + 4.0;
    let top = if below { anchor.bottom() } else { anchor.y };
    let room = (bounds.bottom() - top - 4.0).max(ITEM_HEIGHT * 3.0);
    let (h, rows) = if full <= room {
        (full, menu.items.len())
    } else {
        let rows = ((room - 4.0) / ITEM_HEIGHT).floor() as usize;
        (rows as f32 * ITEM_HEIGHT + 4.0, rows)
    };
    let x = anchor.x.min(bounds.right() - w - 2.0).max(bounds.x);
    OpenMenu {
        menu: 0,
        rect: Rect::new(x, top, w, h),
        hot: None,
        scroll: 0,
        visible_rows: rows,
    }
}

/// Place a submenu beside its parent row, flipping left at the window edge.
pub fn place_side(label: &mut Label, menu: &Menu, parent: Rect, bounds: Rect) -> OpenMenu {
    // Size from the child labels, not the much wider parent menu.
    let sizing_anchor = Rect::new(parent.x, parent.y, 60.0, parent.h);
    let mut open = place(label, menu, sizing_anchor, bounds, false);
    open.rect.x = if parent.right() + open.rect.w <= bounds.right() {
        parent.right() - 1.0
    } else {
        (parent.x - open.rect.w + 1.0).max(bounds.x)
    };
    open
}

/// True when this open menu shows scroll arrows.
fn scrolls(menu: &Menu, open: &OpenMenu) -> bool {
    open.visible_rows < menu.items.len()
}

/// Item rectangles currently visible, with their indices.
fn visible_items(menu: &Menu, open: &OpenMenu) -> Vec<(usize, Rect)> {
    let r = open.rect;
    let mut y = r.y + 2.0;
    let mut out = Vec::new();
    let scrolling = scrolls(menu, open);
    let (first, mut rows) = (open.scroll, open.visible_rows);
    if scrolling {
        // One row each for the up and down arrows when they are needed.
        if open.scroll > 0 {
            y += ITEM_HEIGHT;
            rows -= 1;
        }
        if first + rows < menu.items.len() {
            rows -= 1;
        }
    }
    for (i, it) in menu.items.iter().enumerate().skip(first).take(rows) {
        let h = item_height(it);
        out.push((i, Rect::new(r.x, y, r.w, h)));
        y += h;
    }
    out
}

/// The on-screen row for a visible item.
pub fn item_rect(menu: &Menu, open: &OpenMenu, index: usize) -> Option<Rect> {
    visible_items(menu, open)
        .into_iter()
        .find_map(|(i, rect)| (i == index).then_some(rect))
}

/// The item under a point, if it is a selectable one.
pub fn item_at(menu: &Menu, open: &OpenMenu, x: f32, y: f32) -> Option<usize> {
    if !open.rect.contains(x, y) {
        return None;
    }
    visible_items(menu, open)
        .into_iter()
        .find(|(_, r)| r.contains(x, y))
        .map(|(i, _)| i)
        .filter(|&i| !menu.items[i].separator && menu.items[i].enabled)
}

/// Scroll a tall menu when the pointer rests on one of its arrow rows.
/// Returns true when it scrolled.
pub fn autoscroll(menu: &Menu, open: &mut OpenMenu, y: f32) -> bool {
    if !scrolls(menu, open) || !open.rect.contains(open.rect.x + 1.0, y) {
        return false;
    }
    if open.scroll > 0 && y < open.rect.y + 2.0 + ITEM_HEIGHT {
        open.scroll -= 1;
        return true;
    }
    let max = menu.items.len() - open.visible_rows + 1;
    if open.scroll < max && y > open.rect.bottom() - 2.0 - ITEM_HEIGHT {
        open.scroll += 1;
        return true;
    }
    false
}

/// Scroll a tall menu by item rows, as from a wheel or trackpad.
/// Returns true when the visible portion changed.
pub fn scroll_by(menu: &Menu, open: &mut OpenMenu, rows: i32) -> bool {
    if !scrolls(menu, open) || rows == 0 {
        return false;
    }
    // A scroll arrow occupies a row once the first item has moved, hence
    // the extra position compared with a plain list viewport.
    let max = menu.items.len().saturating_sub(open.visible_rows) + 1;
    let next = (open.scroll as i64 + rows as i64).clamp(0, max as i64) as usize;
    let changed = next != open.scroll;
    open.scroll = next;
    changed
}

pub fn draw_menu(c: &mut Canvas, label: &mut Label, menu: &Menu, open: &OpenMenu) {
    draw_menu_face(c, label, menu, open, interface_font(), BOLD);
}

pub fn draw_context_menu(c: &mut Canvas, label: &mut Label, menu: &Menu, open: &OpenMenu) {
    let font = interface_font();
    draw_menu_face(c, label, menu, open, font, false);
}

fn draw_menu_face(
    c: &mut Canvas,
    label: &mut Label,
    menu: &Menu,
    open: &OpenMenu,
    font: &str,
    bold: bool,
) {
    let r = open.rect;
    c.panel_shadow(r, 6.0);
    round_rect(c, r, 6.0, 0xB8B8B8);
    let hair = c.hair();
    round_rect(c, r.inset(hair), 5.0, 0xF3F3F3);
    let items = visible_items(menu, open);
    let scrolling = scrolls(menu, open);
    if scrolling && open.scroll > 0 {
        arrow_row(c, Rect::new(r.x, r.y + 2.0, r.w, ITEM_HEIGHT), true);
    }
    if scrolling && items.last().is_some_and(|(i, _)| i + 1 < menu.items.len()) {
        arrow_row(
            c,
            Rect::new(r.x, r.bottom() - 2.0 - ITEM_HEIGHT, r.w, ITEM_HEIGHT),
            false,
        );
    }
    for (i, ir) in items {
        let it = &menu.items[i];
        if it.separator {
            c.hline(ir.x + 8.0, ir.right() - 8.0, ir.y + 3.0, 0xD0D0D0);
            continue;
        }
        let hot = open.hot == Some(i);
        if hot {
            round_rect(
                c,
                Rect::new(ir.x + 3.0, ir.y, ir.w - 6.0, ir.h),
                3.0,
                0x0A84FF,
            );
        }
        let fg = if !it.enabled {
            color::GRAY_DARK
        } else if hot {
            color::WHITE
        } else {
            color::BLACK
        };
        let base = ir.y + 12.5;
        if it.checked {
            label.draw(c, font, SIZE, bold, ir.x + 4.0, base, "\u{2713}", fg);
        }
        let x = ir.x + 16.0;
        // Only visible rows reach this point, so a long installed-font list
        // remains lazy: opening it does not load every font on the machine.
        let w = if let Some(font) = it.font.as_deref() {
            if it.style.italic {
                label.draw_italic(c, font, SIZE, it.style.bold, x, base, &it.label, fg)
            } else {
                label.draw(c, font, SIZE, it.style.bold, x, base, &it.label, fg)
            }
        } else {
            draw_styled(c, label, font, &it.label, it.style, bold, x, base, fg)
        };
        let _ = w;
        if !it.key.is_empty() {
            let key = crate::commands::display_key(&it.key);
            let kw = label.width(font, SIZE, bold, &key);
            label.draw(c, font, SIZE, bold, ir.right() - kw - 10.0, base, &key, fg);
        }
        if it.submenu.is_some() {
            let cx = ir.right() - 8.0;
            let cy = ir.y + ir.h / 2.0;
            for i in 0..4 {
                let i = i as f32;
                c.fill(Rect::new(cx + 1.5 - i, cy - i, 1.0, i * 2.0 + 1.0), fg);
            }
        }
    }
}

fn arrow_row(c: &mut Canvas, r: Rect, up: bool) {
    let cx = r.x + r.w / 2.0;
    let cy = r.y + r.h / 2.0;
    for i in 0..5 {
        let i = i as f32;
        let y = if up { cy - 2.0 + i } else { cy + 2.0 - i };
        c.fill(Rect::new(cx - i, y, 2.0 * i + 1.0, 1.0), color::BLACK);
    }
}

/// Draw a label in a style, the way the Format menu shows Bold, Italic and
/// Underline in their own style. Returns the width.
#[allow(clippy::too_many_arguments)]
pub fn draw_styled(
    c: &mut Canvas,
    label: &mut Label,
    font: &str,
    text: &str,
    style: ItemStyle,
    base_bold: bool,
    x: f32,
    base: f32,
    fg: u32,
) -> f32 {
    let bold = base_bold || style.bold;
    let w = if style.italic {
        label.draw_italic(c, font, SIZE, bold || style.bold, x, base, text, fg)
    } else if style.outline {
        for (dx, dy) in [(-0.7, 0.0), (0.7, 0.0), (0.0, -0.7), (0.0, 0.7)] {
            label.draw(c, font, SIZE, bold, x + dx, base + dy, text, fg);
        }
        label.draw(c, font, SIZE, bold, x, base, text, color::GRAY)
    } else {
        let w = label.draw(c, font, SIZE, bold, x, base, text, fg);
        if style.bold {
            label.draw(c, font, SIZE, bold, x + 0.7, base, text, fg);
        }
        w
    };
    if style.underline {
        c.hline(x, x + w, base + 2.0, fg);
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_actions_do_not_have_toggle_state() {
        assert!(!Item::new("Paste", 1).checkable);
        let bold = Item::new("Bold", 2).checked(false);
        assert!(bold.checkable);
        assert!(!bold.checked);
    }

    fn tall_menu(count: usize) -> (Menu, OpenMenu) {
        let menu = Menu {
            title: "Font".into(),
            items: (0..count)
                .map(|i| Item::new(format!("Font {i}"), i as u32))
                .collect(),
        };
        let open = OpenMenu {
            rect: Rect::new(0.0, 0.0, 180.0, 164.0),
            visible_rows: 10,
            ..Default::default()
        };
        (menu, open)
    }

    #[test]
    fn wheel_scrolls_a_tall_menu_and_clamps_at_both_ends() {
        let (menu, mut open) = tall_menu(100);
        assert!(scroll_by(&menu, &mut open, 3));
        assert_eq!(open.scroll, 3);
        assert!(scroll_by(&menu, &mut open, i32::MAX));
        assert_eq!(open.scroll, 91);
        assert!(!scroll_by(&menu, &mut open, 1));
        assert!(scroll_by(&menu, &mut open, i32::MIN));
        assert_eq!(open.scroll, 0);
    }

    #[test]
    fn short_menus_do_not_scroll() {
        let (menu, mut open) = tall_menu(6);
        assert!(!scroll_by(&menu, &mut open, 3));
        assert_eq!(open.scroll, 0);
    }

    #[test]
    fn font_preview_is_opt_in_per_item() {
        let plain = Item::new("Plain", 1);
        let preview = Item::new("Georgia", 2).font("Georgia");
        assert_eq!(plain.font, None);
        assert_eq!(preview.font.as_deref(), Some("Georgia"));
    }
}
