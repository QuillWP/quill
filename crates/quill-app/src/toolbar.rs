//! The standard toolbar: the row of buttons under the menu bar for files,
//! editing, tools, inserting and views.
//!
//! The pictures are Quill's own: flat vector drawings on a 19 x 18 grid (a
//! few borrowed from the old ribbon on 16 x 16; see `icon`), drawn at the
//! screen's full resolution. Buttons are flat too, with no frame; the one
//! under the pointer and the one held down get a rounded backing.

use std::cell::RefCell;

use crate::bars::{background, hover_color, line_color, padding, pressed_color};
use crate::canvas::{self, Canvas, Label, Rect};
use crate::commands::Cmd;
use crate::icon::{self, Image};

/// The bar's thickness: an icon with padding on each side and its divider.
pub fn thickness(vertical: bool) -> f32 {
    let vert = padding().1;
    if vertical {
        2.0 * vert + ICON_W as f32 + 2.0
    } else {
        2.0 * vert + ICON_H as f32 + 1.0
    }
}

/// Button pitch from one icon to the next.
fn pitch() -> f32 {
    34.0
}
const ICON_W: u32 = 19;
const ICON_H: u32 = 18;
const OVERFLOW_W: f32 = 24.0;

fn group_gap() -> f32 {
    15.0
}

#[derive(Clone, Copy)]
struct Tool {
    /// Balloon Help name of the original button.
    name: &'static str,
    shapes: &'static str,
    /// The grid the shapes are drawn on, in points.
    grid: (u32, u32),
    /// None for buttons whose feature comes in a later milestone.
    cmd: Option<Cmd>,
}

const fn tool(name: &'static str, shapes: &'static str, cmd: Option<Cmd>) -> Tool {
    Tool {
        name,
        shapes,
        grid: (ICON_W, ICON_H),
        cmd,
    }
}

/// A picture drawn on the old ribbon's 16 x 16 grid.
const fn small(name: &'static str, shapes: &'static str, cmd: Option<Cmd>) -> Tool {
    Tool {
        name,
        shapes,
        grid: (16, 16),
        cmd,
    }
}

/// Every picture, with the names Balloon Help gives them. Customize saves
/// pictures by index, so entries are only ever added at the end.
const TOOLS: &[Tool] = &[
    tool("New", NEW_S, Some(Cmd::New)),
    tool("Open", OPEN_S, Some(Cmd::Open)),
    tool("Save", SAVE_S, Some(Cmd::Save)),
    tool("Print", PRINT_S, Some(Cmd::Print)),
    tool("Cut", CUT_S, Some(Cmd::Cut)),
    tool("Copy", COPY_S, Some(Cmd::Copy)),
    tool("Paste", PASTE_S, Some(Cmd::Paste)),
    tool("Undo", UNDO_S, Some(Cmd::Undo)),
    tool("Bullet", BULLET_S, Some(Cmd::Bullet)),
    tool("Unnest Paragraph", UNNEST_S, Some(Cmd::Unnest)),
    tool("Nest Paragraph", NEST_S, Some(Cmd::Nest)),
    tool("Spelling", SPELLING_S, Some(Cmd::Spelling)),
    tool("Change Case", CHANGE_CASE_S, Some(Cmd::ChangeCase)),
    tool("Border", BORDER_S, Some(Cmd::BorderDialog)),
    tool("Up", UP_S, Some(Cmd::FontSizeUp)),
    tool("Down", DOWN_S, Some(Cmd::FontSizeDown)),
    tool("Replace", REPLACE_S, Some(Cmd::Replace)),
    tool("Page Break", PAGE_BREAK_S, Some(Cmd::PageBreak)),
    tool("Normal", NORMAL_VIEW_S, Some(Cmd::NormalView)),
    tool("Page Layout", PAGE_LAYOUT_S, Some(Cmd::PageLayoutView)),
    tool("Print Preview", PRINT_PREVIEW_S, Some(Cmd::PrintPreview)),
    small("Insert Table", TABLE_S, Some(Cmd::InsertTable)),
    small("Insert Picture", PICTURE_S, Some(Cmd::InsertPicture)),
    small("Show/Hide ¶", MARKS_S, Some(Cmd::ShowMarks)),
    tool("Hyperlink", LINK_S, Some(Cmd::Hyperlink)),
    tool("Remove Link", UNLINK_S, Some(Cmd::RemoveHyperlink)),
];

/// The standard toolbar's buttons, left to right: a picture in `TOOLS`,
/// and whether a new group starts there.
const LAYOUT: &[(usize, bool)] = &[
    // New, Open, Save, Print
    (0, false),
    (1, false),
    (2, false),
    (3, false),
    // Cut, Copy, Paste, Undo
    (4, true),
    (5, false),
    (6, false),
    (7, false),
    // Spelling, Replace
    (11, true),
    (16, false),
    // Page Break
    (17, true),
    // Show/Hide ¶, Normal, Page Layout, Print Preview
    (23, true),
    (18, false),
    (19, false),
    (20, false),
];

/// Where the toolbar is: its top left corner, and whether it runs down
/// the window (docked left or right) rather than across.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub x: f32,
    pub y: f32,
    pub vertical: bool,
}

/// Each button's icon, in points. The first starts at the horizontal padding,
/// and each sits at the vertical padding like the formatting toolbar controls.
fn rects(at: Place) -> Vec<Rect> {
    let (horiz, vert) = padding();
    let (w, h) = (ICON_W as f32, ICON_H as f32);
    let mut d = horiz;
    LAYOUT
        .iter()
        .map(|&(_, group)| {
            if group {
                d += group_gap();
            }
            let r = if at.vertical {
                Rect::new(at.x + 1.0 + vert, at.y + d, w, h)
            } else {
                Rect::new(at.x + d, at.y + vert, w, h)
            };
            d += pitch();
            r
        })
        .collect()
}

/// The extra button at the trailing edge when a horizontal toolbar cannot
/// show every item. Its index follows the real toolbar slots.
pub fn overflow_index() -> usize {
    LAYOUT.len()
}

/// The hidden slots, reserving room for the overflow button when needed.
pub fn hidden_slots(at: Place, length: f32) -> Vec<usize> {
    if at.vertical {
        return Vec::new();
    }
    let all = rects(at);
    if all
        .last()
        .is_none_or(|r| r.right() <= at.x + length - padding().0)
    {
        return Vec::new();
    }
    let cutoff = at.x + length - padding().0 - OVERFLOW_W - 4.0;
    all.iter()
        .enumerate()
        .filter_map(|(i, r)| (r.right() > cutoff).then_some(i))
        .collect()
}

/// The overflow button's frame, including its hit target.
pub fn overflow_rect(at: Place, length: f32) -> Option<Rect> {
    (!hidden_slots(at, length).is_empty()).then(|| {
        Rect::new(
            at.x + length - padding().0 - OVERFLOW_W,
            at.y,
            OVERFLOW_W,
            thickness(false) - 1.0,
        )
    })
}

/// Button `i`'s picture and command: the original's own, or as Customize set it.
pub fn slot(i: usize) -> (usize, Option<Cmd>) {
    let custom = crate::settings::with(|s| {
        s.toolbar_buttons
            .iter()
            .find(|(p, _, c)| *p == i && crate::commands::by_name(c).is_some())
            .map(|(_, pic, c)| (*pic, crate::commands::by_name(c)))
    });
    let default = LAYOUT.get(i).map_or(0, |l| l.0);
    let (picture, command) = custom.unwrap_or((default, TOOLS.get(default).and_then(|t| t.cmd)));
    let command = command.filter(|cmd| *cmd != Cmd::RepaginateNow);
    (picture, command)
}

/// How many buttons there are.
pub fn positions() -> usize {
    LAYOUT.len()
}

/// How many pictures there are to choose from.
pub fn pictures() -> usize {
    TOOLS.len()
}

/// Picture `i`'s name, for Customize.
pub fn picture_name(i: usize) -> &'static str {
    TOOLS.get(i).map_or("", |t| t.name)
}

/// A picture by its Balloon Help name, for the other toolbars' buttons that
/// share the standard toolbar's pictures: its shapes, grid and command.
pub fn picture_named(name: &str) -> Option<(&'static str, (u32, u32), Option<Cmd>)> {
    TOOLS
        .iter()
        .find(|t| t.name == name)
        .map(|t| (t.shapes, t.grid, t.cmd))
}

/// What the toolbar needs to know to show its buttons' states.
pub struct ToolState {
    pub has_selection: bool,
    pub can_undo: bool,
    /// Show/Hide ¶ is on.
    pub show_marks: bool,
}

fn enabled(cmd: Option<Cmd>, s: &ToolState) -> bool {
    match cmd {
        None => false,
        Some(Cmd::Cut | Cmd::Copy) => s.has_selection,
        Some(Cmd::Undo) => s.can_undo,
        Some(_) => true,
    }
}

pub fn is_enabled(i: usize, state: &ToolState) -> bool {
    enabled(slot(i).1, state)
}

pub fn slot_name(i: usize) -> &'static str {
    let (picture, command) = slot(i);
    command
        .map(crate::commands::tip_name)
        .or_else(|| TOOLS.get(picture).map(|tool| tool.name))
        .unwrap_or("")
}

/// Icons and the (scale, bar color) they were rendered for.
type IconCache = ((f32, u32), Vec<Image<'static>>);

thread_local! {
    /// The icons as last rendered.
    static ICONS: RefCell<Option<IconCache>> = const { RefCell::new(None) };
}

/// Run `f` with every picture rendered for `n` device pixels a point.
fn with_icons<R>(n: f32, f: impl FnOnce(&[Image<'static>]) -> R) -> R {
    ICONS.with(|cell| {
        let mut cache = cell.borrow_mut();
        let key = (n, background());
        if cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let made = TOOLS
                .iter()
                .map(|t| icon::render(t.shapes, t.grid.0, t.grid.1, n * fit(t), key.1))
                .collect();
            *cache = Some((key, made));
        }
        f(&cache.as_ref().unwrap().1)
    })
}

/// How much picture `t` is enlarged to stand as tall as the others.
fn fit(t: &Tool) -> f32 {
    ICON_H as f32 / t.grid.1 as f32
}

/// The rounded backing behind a button's picture: a little round the icon.
fn backing(r: Rect) -> Rect {
    let x = r.x - (pitch() - r.w) / 2.0 + 2.0;
    Rect::new(x, r.y - 4.0, pitch() - 4.0, r.h + 8.0)
}

/// Where a click counts for the button with icon `r`: its share of the
/// bar's length, across the bar's whole thickness.
fn cell(at: Place, r: Rect) -> Rect {
    let vert = padding().1;
    if at.vertical {
        let y = r.y - (pitch() - r.h) / 2.0;
        Rect::new(at.x, y, thickness(true), pitch())
    } else {
        let x = r.x - (pitch() - r.w) / 2.0;
        Rect::new(x, r.y - vert, pitch(), thickness(false) - 1.0)
    }
}

/// Draw the toolbar at `at`, `length` long (its width across the window,
/// or its height down it), with button `hover` under the pointer and
/// `pressed` held down.
pub fn draw(
    c: &mut Canvas,
    label: &mut Label,
    at: Place,
    length: f32,
    s: &ToolState,
    hover: Option<usize>,
    pressed: Option<usize>,
) {
    if at.vertical {
        let t = thickness(true);
        c.fill(Rect::new(at.x, at.y, t, length), background());
        // Hairlines, the same thickness as the formatting toolbar's dividers.
        let hair = c.hair();
        c.fill(Rect::new(at.x, at.y, hair, length), line_color());
        c.fill(Rect::new(at.x + t - hair, at.y, hair, length), line_color());
    } else {
        let t = thickness(false);
        c.fill(Rect::new(at.x, at.y, length, t), background());
        // The divider keeps the bar's side padding, like the buttons.
        let pad = padding().0;
        let hair = c.hair();
        c.hline(
            at.x + pad,
            at.x + length - pad,
            at.y + t - hair,
            line_color(),
        );
    }
    let clip = c.set_clip(if at.vertical {
        Rect::new(at.x, at.y, thickness(true), length)
    } else {
        Rect::new(at.x, at.y, length, thickness(false))
    });
    let button_rects = rects(at);
    for (i, r) in button_rects.iter().enumerate().skip(1) {
        if !LAYOUT[i].1 {
            continue;
        }
        if at.vertical {
            let y = r.y - group_gap() / 2.0;
            c.hline(at.x + 8.0, at.x + thickness(true) - 8.0, y, line_color());
        } else {
            let x = r.x - group_gap() / 2.0;
            c.vline(x, at.y + 7.0, at.y + thickness(false) - 7.0, line_color());
        }
    }
    let hidden = hidden_slots(at, length);
    let n = c.scale;
    with_icons(n, |images| {
        for (i, r) in button_rects.into_iter().enumerate() {
            if hidden.contains(&i) {
                continue;
            }
            if r.w == 0.0 {
                continue;
            }
            if r.x > at.x + length || r.y > at.y + length {
                break;
            }
            let (pic, cmd) = slot(i);
            let Some(im) = images.get(pic) else {
                continue;
            };
            let live = enabled(cmd, s);
            let on = cmd == Some(Cmd::ShowMarks) && s.show_marks;
            if pressed == Some(i) || on {
                canvas::round_rect(c, backing(r), 5.0, pressed_color());
            } else if hover == Some(i) && live {
                canvas::round_rect(c, backing(r), 5.0, hover_color());
            }
            // Buttons that can do nothing just now show faded.
            let alpha = if live { 255 } else { c.faded() };
            let t = &TOOLS[pic];
            let k = fit(t);
            let x = r.x + (r.w - t.grid.0 as f32 * k) / 2.0;
            // Draw a smaller grid's picture enlarged, letters included.
            let old = c.zoom(k);
            icon::paint(c, label, im, x / k, r.y / k, alpha, background());
            c.set_scale(old);
        }
    });
    if let Some(r) = overflow_rect(at, length) {
        if pressed == Some(overflow_index()) {
            canvas::round_rect(c, r.inset(2.0), 5.0, pressed_color());
        } else if hover == Some(overflow_index()) {
            canvas::round_rect(c, r.inset(2.0), 5.0, hover_color());
        }
        let im = icon::cached(OVERFLOW_S, 16, 16, c.scale);
        icon::paint(
            c,
            label,
            &im,
            r.x + (r.w - 16.0) / 2.0,
            r.y + (r.h - 16.0) / 2.0,
            255,
            background(),
        );
    }
    c.restore_clip(clip);
}

/// The button under a point, by index, whether it can be used now or not:
/// a faded button still says what it is.
pub fn button_at(at: Place, length: f32, x: f32, y: f32) -> Option<usize> {
    if overflow_rect(at, length).is_some_and(|r| r.contains(x, y)) {
        return Some(overflow_index());
    }
    let hidden = hidden_slots(at, length);
    rects(at)
        .iter()
        .enumerate()
        .find(|(i, r)| !hidden.contains(i) && r.w > 0.0 && cell(at, **r).contains(x, y))
        .map(|(i, _)| i)
}

/// The enabled button under a point, by index.
pub fn hit(at: Place, length: f32, x: f32, y: f32, s: &ToolState) -> Option<usize> {
    button_at(at, length, x, y).filter(|&i| i == overflow_index() || enabled(slot(i).1, s))
}

/// Button `i`'s tooltip: what its command is called, and its shortcut.
pub fn tip(i: usize) -> Option<String> {
    if i == overflow_index() {
        return Some("More Toolbar Items".into());
    }
    let (pic, cmd) = slot(i);
    match cmd {
        Some(c) => Some(crate::commands::tip(crate::commands::tip_name(c), Some(c))),
        None => TOOLS.get(pic).map(|t| t.name.to_string()),
    }
}

pub fn command(i: usize) -> Option<Cmd> {
    slot(i).1
}

// The pictures, one shape per line (see `icon`).

const NEW_S: &str = "
    poly W k 4 1 11 1 15 5 15 16 4 16
    line k 11 1 11 5 15 5
";
const OPEN_S: &str = "
    poly y k 2 3 7 3 9 5 16 5 16 15 2 15
    poly Y k 2 15 4.5 8 18 8 16 15
";
const SAVE_S: &str = "
    rrect B - 2 1 16 16 2
    rect W 5 1 12 5
    rect B 10 2 11 4
    rrect W - 5 9 13 16 1
    line s 7 11 11 11
    line s 7 13 11 13
";
const PRINT_S: &str = "
    rrect W k 5 1 13 6 0.5
    rrect s - 2 5 16 12 2
    rrect W k 5 10 13 16 0.5
    line s 7 13 11 13
    dot G 14 7 0.8
";
const CUT_S: &str = "
    line k 6.8 11.6 12.5 1.5
    line k 11.2 11.6 5.5 1.5
    ring R 5 13.5 2.3 2.3
    ring R 13 13.5 2.3 2.3
";
const COPY_S: &str = "
    rrect W k 3 1 11 12 1
    rrect W k 7 5 15 16 1
    line s 9 9 13 9
    line s 9 11 13 11
    line s 9 13 12 13
";
const PASTE_S: &str = "
    rrect Y k 3 3 15 16 1.5
    rect W 5 6 13 14
    line s 7 9 11 9
    line s 7 11 11 11
    rrect s - 6 1 12 4 1
";
const UNDO_S: &str = "
    line k 5 7 11 7
    arc k 11 11 4 270 450
    line k 11 15 6 15
    poly k - 1.5 7 6 3.5 6 10.5
";
const BULLET_S: &str = "
    dot B 4 4 1.5
    dot B 4 9 1.5
    dot B 4 14 1.5
    line k 8 4 16 4
    line k 8 9 16 9
    line k 8 14 16 14
";
const UNNEST_S: &str = "
    line k 2 2 16 2
    line k 9 6 16 6
    line k 9 10 16 10
    line k 2 14 16 14
    poly B - 1.5 8 6.5 4.5 6.5 11.5
";
const NEST_S: &str = "
    line k 2 2 16 2
    line k 9 6 16 6
    line k 9 10 16 10
    line k 2 14 16 14
    poly B - 6.5 8 1.5 4.5 1.5 11.5
";
const SPELLING_S: &str = "
    text k Helvetica 7 1 1.5 7.5 ABC
    line G/1.5 5 12 8 15 14 9
";
const CHANGE_CASE_S: &str = "
    text k Helvetica 13 0 1 14 A
    text B Helvetica 10 0 10 14 a
";
const BORDER_S: &str = "
    dash s 2 2 16 2 1 1
    dash s 2 2 2 15 1 1
    dash s 16 2 16 15 1 1
    dash s 9 4 9 14 1 1
    dash s 4 9 14 9 1 1
    line B/2 2 16 16 16
";
const UP_S: &str = "
    text k Helvetica 13 1 1 15 A
    poly B - 11.5 7 18.5 7 15 2.5
";
const DOWN_S: &str = "
    text k Helvetica 10 1 2 15 A
    poly B - 11.5 10 18.5 10 15 14.5
";
const REPLACE_S: &str = "
    text k Helvetica 9 1 1 8 A
    text B Helvetica 9 1 11.5 16 B
    line k 4 10 4 13 8 13
    poly k - 8 10.5 10.5 13 8 15.5
";
const PAGE_BREAK_S: &str = "
    rect W 5 0 13 4
    line k 4 0 4 5 14 5 14 0
    dash B 2 8 16 8 2 1
    rect W 5 12 13 17
    line k 4 17 4 11 14 11 14 17
";
const NORMAL_VIEW_S: &str = "
    line k 2 3 16 3
    line k 2 6 16 6
    line k 2 9 11 9
    dash s 2 12 16 12 2 1
    line k 2 15 16 15
";
const PAGE_LAYOUT_S: &str = "
    rrect W k 3 1 15 16 1
    line s 6 4 12 4
    line s 6 7 12 7
    line s 6 10 12 10
    line s 6 13 10 13
";
const PRINT_PREVIEW_S: &str = "
    rrect W k 2 1 12 15 1
    line s 5 4 9 4
    line s 5 7 9 7
    dot W 12 11 3.5
    ring B 12 11 3.2 3.2
    line B/1.5 14.5 13.5 17 16
";
// These four came from the old ribbon and are on its 16 x 16 grid.
const TABLE_S: &str = "
    rrect W k 2 2 14 14 1
    rect b 3 3 13 5
    line k 2 6 14 6
    line k 2 10 14 10
    line k 8 2 8 14
";
const PICTURE_S: &str = "
    rrect W k 2 3 14 13 1
    poly G - 2.5 12.5 7 7 10 10.5 11.5 9 13.5 12.5
    dot Y 11 6 1.3
";
// Two chain links, joined.
const LINK_S: &str = "
    rrect - k/1.5 1.5 6 10.5 12 3
    rrect - B/1.5 8.5 6 17.5 12 3
";
// The same links pulled apart.
const UNLINK_S: &str = "
    rrect - k/1.5 1 6 8 12 3
    rrect - B/1.5 11 6 18 12 3
    line R 9.5 2 9.5 4
    line R 6.5 2.5 7.5 4
    line R 12.5 2.5 11.5 4
    line R 9.5 14 9.5 16
    line R 6.5 15.5 7.5 14
    line R 12.5 15.5 11.5 14
";
const MARKS_S: &str = "
    dot k 6 5.5 2.6
    line k 8 3 8 14
    line k 11 3 11 14
    line k 6 3 12 3
";
const OVERFLOW_S: &str = "
    line k/1.5 4 6 8 10 12 6
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_indices_follow_the_tools_table() {
        assert_eq!(TOOLS.len(), 26);
        assert_eq!(TOOLS[11].name, "Spelling");
        assert_eq!(TOOLS[23].name, "Show/Hide ¶");
    }

    #[test]
    fn the_standard_layout_is_files_editing_tools_inserting_and_views() {
        let names: Vec<&str> = LAYOUT.iter().map(|&(p, _)| TOOLS[p].name).collect();
        assert_eq!(
            names,
            [
                "New",
                "Open",
                "Save",
                "Print",
                "Cut",
                "Copy",
                "Paste",
                "Undo",
                "Spelling",
                "Replace",
                "Page Break",
                "Show/Hide ¶",
                "Normal",
                "Page Layout",
                "Print Preview",
            ]
        );
    }

    #[test]
    fn unknown_commands_use_the_default() {
        let original = crate::settings::get();
        crate::settings::update(|s| {
            s.toolbar_buttons = vec![(8, 0, "UnknownCommand".into())];
        });
        assert!(crate::commands::by_name("UnknownCommand").is_none());
        assert_eq!(slot(8), (11, Some(Cmd::Spelling)));
        crate::settings::update(|s| *s = original);
    }

    #[test]
    fn every_shape_line_parses() {
        let ops = [
            "poly", "rrect", "line", "dash", "arc", "curve", "rect", "box", "bar", "dot", "ring",
            "clip", "text",
        ];
        for t in TOOLS {
            for l in t.shapes.lines().map(str::trim).filter(|l| !l.is_empty()) {
                let op = l.split_whitespace().next().unwrap();
                assert!(ops.contains(&op), "{}: {l}", t.name);
            }
        }
    }

    #[test]
    fn every_picture_draws_something() {
        for t in TOOLS.iter().filter(|t| !t.shapes.is_empty()) {
            let im = icon::render(t.shapes, t.grid.0, t.grid.1, 1.0, background());
            let inked = im.px.iter().filter(|&&p| p >> 24 > 0).count();
            assert!(inked > 10 || !im.texts.is_empty(), "{} is blank", t.name);
        }
    }

    #[test]
    fn buttons_sit_a_pitch_apart_with_gaps_between_groups() {
        let r = rects(Place {
            x: 0.0,
            y: 0.0,
            vertical: false,
        });
        let groups = LAYOUT.iter().filter(|l| l.1).count() as f32;
        assert_eq!(r[0].x, padding().0);
        assert_eq!(r[1].x - r[0].x, pitch());
        assert_eq!(
            r.last().unwrap().right(),
            padding().0
                + groups * group_gap()
                + (LAYOUT.len() - 1) as f32 * pitch()
                + ICON_W as f32
        );
    }

    #[test]
    fn a_narrow_horizontal_toolbar_reserves_an_overflow_button() {
        let at = Place {
            x: 0.0,
            y: 10.0,
            vertical: false,
        };
        assert!(hidden_slots(at, 2000.0).is_empty());
        assert!(overflow_rect(at, 2000.0).is_none());
        let hidden = hidden_slots(at, 360.0);
        assert!(!hidden.is_empty());
        assert!(hidden.contains(&(LAYOUT.len() - 1)));
        let overflow = overflow_rect(at, 360.0).unwrap();
        assert_eq!(
            button_at(at, 360.0, overflow.x + 2.0, overflow.y + 2.0),
            Some(overflow_index())
        );
        assert!(
            hidden_slots(
                Place {
                    vertical: true,
                    ..at
                },
                100.0
            )
            .is_empty()
        );
    }

    #[test]
    fn other_toolbars_find_pictures_by_name() {
        assert_eq!(picture_named("Bullet").map(|p| p.1), Some((19, 18)));
        assert_eq!(picture_named("Insert Table").map(|p| p.1), Some((16, 16)));
        assert!(picture_named("Insert Graph").is_none());
    }
}
