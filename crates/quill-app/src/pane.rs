//! Document editors: comments on the right, headers, footers and notes
//! below the document. Flat panel headings replace the classic window bars.

use crate::canvas::{Canvas, Label, Rect, color};
use crate::docview::DocView;
use crate::icon;
use quill_core::StoryId;
use quill_core::para::chars;

/// Heights of the title strip and the icon row, in interface units.
pub const TITLE: f32 = 32.0;
pub const ICONS: f32 = 30.0;

pub struct Pane {
    pub story: StoryId,
    /// For a header or footer: the section whose it is.
    pub section: usize,
    /// The view that is not in `WinState::view`: the pane's own while the
    /// main text has focus, the main text's while the pane has it.
    pub other: DocView,
    pub focused: bool,
    /// Panel bounds in interface units, shared by painting and input.
    pub bounds: Rect,
}

/// What a click in the pane's furniture hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneHit {
    Close,
    /// Insert this field character.
    Field(char),
    /// Same As Previous: share the section before's headers and footers.
    SameAsPrevious,
    /// Panel heading or unused control space; do not move the caret.
    Chrome,
    /// The text, or nothing in particular.
    Text,
}

/// The Same As Previous button, in the icon row.
fn same_box(top: f32) -> Rect {
    Rect::new(292.0, top + TITLE + 5.0, 120.0, 19.0)
}

/// Labeled field buttons fit beside Same As Previous even at the
/// window's minimum width. Drawing and hit testing share their bounds.
const FIELD_ICONS: [(f32, f32, char, &str, &str); 3] = [
    (8.0, 116.0, chars::FIELD_PAGE, PAGE_ICON, "Page Number"),
    (130.0, 72.0, chars::FIELD_DATE, DATE_ICON, "Date"),
    (208.0, 72.0, chars::FIELD_TIME, TIME_ICON, "Time"),
];

fn field_box(top: f32, x: f32, width: f32) -> Rect {
    Rect::new(x, top + TITLE + 3.0, width, ICONS - 6.0)
}

fn close_box(top: f32) -> Rect {
    Rect::new(0.0, top + 6.0, 20.0, 20.0)
}

/// Comments use the full document height, leaving room for the body even
/// in the minimum-size window. Other stories keep a bottom editor.
pub fn bounds(story: StoryId, area: Rect) -> Rect {
    if story == StoryId::Annotations {
        let width = (area.w * 0.4).clamp(200.0, 420.0).min(area.w * 0.5);
        Rect::new(area.right() - width, area.y, width, area.h)
    } else {
        let height = (area.h * 0.4)
            .clamp(90.0, 240.0)
            .min((area.h - 40.0).max(0.0));
        Rect::new(area.x, area.bottom() - height, area.w, height)
    }
}

/// The height of the icon row: the footnote pane has none.
pub fn icons(story: StoryId) -> f32 {
    if matches!(story, StoryId::Notes | StoryId::Annotations) {
        0.0
    } else {
        ICONS
    }
}

/// What a click at (`cx`, `cy`) hits; `same` is whether Same As Previous
/// can be clicked (a later section with headers of its own).
pub fn hit(bounds: Rect, story: StoryId, same: bool, cx: f32, cy: f32) -> PaneHit {
    let (cx, cy) = (cx - bounds.x, cy - bounds.y);
    let top = 0.0;
    if cy < top + TITLE {
        if Rect::new(bounds.w - 30.0, 6.0, 20.0, 20.0).contains(cx, cy) {
            return PaneHit::Close;
        }
        return PaneHit::Chrome;
    }
    if cy < top + TITLE + icons(story) {
        if same && same_box(top).contains(cx, cy) {
            return PaneHit::SameAsPrevious;
        }
        for (x, width, c, _, _) in FIELD_ICONS {
            if field_box(top, x, width).contains(cx, cy) {
                return PaneHit::Field(c);
            }
        }
        return PaneHit::Chrome;
    }
    PaneHit::Text
}

/// The title strip and icon row, `width` wide from `top`.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    c: &mut Canvas,
    label: &mut Label,
    bounds: Rect,
    title: &str,
    _focused: bool,
    story: StoryId,
    same: bool,
) {
    let (top, width, left) = (bounds.y, bounds.w, bounds.x);
    let clip = c.set_clip(bounds);
    let strip = Rect::new(left, top, width, TITLE);
    c.fill(strip, 0xF3F3F3);
    c.hline(left, bounds.right(), top, 0xD0D0D0);
    c.hline(left, bounds.right(), strip.bottom() - c.hair(), 0xD0D0D0);
    c.vline(left, top, bounds.bottom(), 0xD0D0D0);
    label.draw(
        c,
        quill_layout::fonts::UI_FONT,
        13.0,
        true,
        left + 12.0,
        top + 21.0,
        title,
        color::BLACK,
    );
    let b = Rect::new(bounds.right() - 30.0, close_box(top).y, 20.0, 20.0);
    label.draw(
        c,
        quill_layout::fonts::UI_FONT,
        16.0,
        false,
        b.x + 5.0,
        b.y + 15.0,
        "×",
        0x555555,
    );
    if icons(story) == 0.0 {
        c.restore_clip(clip);
        return;
    }

    let row = Rect::new(left, strip.bottom(), width, ICONS);
    c.fill(row, 0xFAFAFA);
    c.hline(left, bounds.right(), row.bottom() - c.hair(), 0xD0D0D0);
    let n = c.scale;
    for (x, width, _, spec, text) in FIELD_ICONS {
        let b = field_box(top, left + x, width);
        c.fill(b, color::WHITE);
        c.frame(b, 0xCCCCCC);
        let im = icon::render(spec, 20, 20, n, color::WHITE);
        let y = row.y + (ICONS - 20.0) / 2.0;
        icon::paint(c, label, &im, left + x + 3.0, y, 255, color::WHITE);
        label.draw(
            c,
            quill_layout::fonts::UI_FONT,
            12.0,
            true,
            left + x + 27.0,
            row.y + 19.0,
            text,
            color::BLACK,
        );
    }
    // Same As Previous: for a later section with headers of its own.
    let mut b = same_box(top);
    b.x += left;
    let ink = if same { color::BLACK } else { color::GRAY_DARK };
    c.fill(b, color::WHITE);
    c.frame(b, 0xCCCCCC);
    let text = "Same As Previous";
    let w = label.width(quill_layout::fonts::UI_FONT, 12.0, true, text);
    label.draw(
        c,
        quill_layout::fonts::UI_FONT,
        12.0,
        true,
        b.x + (b.w - w) / 2.0,
        b.y + 14.0,
        text,
        ink,
    );
    c.restore_clip(clip);
}

// The field icons.

const PAGE_ICON: &str = "
    rrect W k 3 1 15 18 1
    line B 8 6 7 14
    line B 12 6 11 14
    line B 5 8 14 8
    line B 4.5 12 13.5 12
";
const DATE_ICON: &str = "
    rrect W k 2 3 18 18 1.5
    rect R 3 4 17 7
    line k 6 1 6 4
    line k 14 1 14 4
    dot s 6 11 0.9
    dot s 10 11 0.9
    dot s 14 11 0.9
    dot s 6 15 0.9
    dot s 10 15 0.9
";
const TIME_ICON: &str = "
    dot W 10 10 8
    ring k 10 10 8 8
    line k 10 5 10 10 13.5 12
    dot B 10 10 1.2
";
