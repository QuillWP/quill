//! Outline view's icon bar, in the ruler's place: promote, demote, move
//! up and down, make body text, expand, collapse, show heading levels 1
//! to 4, show all, first line only and show formatting, at the original's
//! positions with Quill's own icons.

use crate::canvas::{self, Canvas, Label, Rect, color};
use crate::commands::Cmd;
use crate::icon;

pub const HEIGHT: f32 = 20.0;

/// Every icon sits on a 12 x 12 grid (see `icon`).
const SIZE: f32 = 12.0;

const LEFT: &str = "
    line k 3 6 11 6
    poly k - -0.5 6 4.5 1 4.5 11
";
const RIGHT: &str = "
    line k 0 6 8 6
    poly k - 11.5 6 6.5 1 6.5 11
";
const UP: &str = "
    line k 6 3 6 11
    poly k - 6 -0.5 1 4.5 11 4.5
";
const DOWN: &str = "
    line k 6 0 6 8
    poly k - 6 11.5 1 6.5 11 6.5
";
const BODY: &str = "
    poly B - -0.5 1.5 4 6 -0.5 10.5
    line k 6 2 11 2
    line k 6 6 11 6
    line k 6 10 11 10
";
const PLUS: &str = "
    rect k 1 5 10 6
    rect k 5 1 6 10
";
const MINUS: &str = "
    rect k 1 5 10 6
";
const ONE: &str = "
    text k Helvetica 11 1 3 10.5 1
";
const TWO: &str = "
    text k Helvetica 11 1 3 10.5 2
";
const THREE: &str = "
    text k Helvetica 11 1 3 10.5 3
";
const FOUR: &str = "
    text k Helvetica 11 1 3 10.5 4
";
const ALL: &str = "
    line k 0 1 11 1
    line k 2 4 11 4
    line s 2 7 11 7
    line s 2 10 9 10
";
const FIRST: &str = "
    line k 0 2 11 2
    line s 2 6 7 6
    dot s 9 6 0.6
    dot s 11 6 0.6
    line k 0 10 11 10
";
const FORMAT: &str = "
    text k Times 12 0 1 10 A
    line B 0 11 11 11
";

/// The icons: where each starts across (from the window's left edge),
/// its picture and its command.
const ICONS: [(f32, &str, Cmd); 14] = [
    (17.0, LEFT, Cmd::PromoteHeading),
    (45.0, RIGHT, Cmd::DemoteHeading),
    (74.0, UP, Cmd::MoveHeadingUp),
    (102.0, DOWN, Cmd::MoveHeadingDown),
    (128.0, BODY, Cmd::MakeBodyText),
    (158.0, PLUS, Cmd::ExpandSubtext),
    (186.0, MINUS, Cmd::CollapseSubtext),
    (216.0, ONE, Cmd::ShowHeading1),
    (243.0, TWO, Cmd::ShowHeading2),
    (271.0, THREE, Cmd::ShowHeading3),
    (299.0, FOUR, Cmd::ShowHeading4),
    (325.0, ALL, Cmd::ShowBodyText),
    (351.0, FIRST, Cmd::FirstLineOnly),
    (379.0, FORMAT, Cmd::ShowFormatting),
];

/// Draw the bar `width` wide from `top`. `first_line` and `formatting`
/// are the two toggles; a toggle that is on (or, for Show Formatting,
/// off) gets a highlighted backing.
pub fn draw(
    c: &mut Canvas,
    label: &mut Label,
    top: f32,
    width: f32,
    first_line: bool,
    formatting: bool,
) {
    let bar = Rect::new(0.0, top, width, HEIGHT);
    c.fill(bar, color::WHITE);
    c.hline(0.0, width, top, color::BLACK);
    c.hline(0.0, width, bar.bottom() - 1.0, color::BLACK);
    // Ticks along the bottom mark the levels, 40 points apart.
    let mut x = 28.0;
    while x < width {
        c.fill(Rect::new(x, bar.bottom() - 3.0, 1.0, 2.0), color::BLACK);
        x += 40.0;
    }
    let n = c.scale;
    let y = top + (HEIGHT - SIZE) / 2.0;
    for (x, spec, cmd) in ICONS {
        let on = match cmd {
            Cmd::FirstLineOnly => first_line,
            Cmd::ShowFormatting => !formatting,
            _ => false,
        };
        if on {
            canvas::round_rect(
                c,
                Rect::new(x - 3.0, top + 2.0, SIZE + 6.0, HEIGHT - 4.0),
                3.0,
                color::HIGHLIGHT,
            );
        }
        let im = icon::render(spec, SIZE as u32, SIZE as u32, n, color::WHITE);
        icon::paint(c, label, &im, x, y, 255, color::WHITE);
    }
}

/// The command under (`x`, `y`) in the bar starting at `top`.
pub fn hit(top: f32, x: f32, y: f32) -> Option<Cmd> {
    if y < top || y >= top + HEIGHT {
        return None;
    }
    ICONS
        .iter()
        .find(|(ix, _, _)| x >= ix - 6.0 && x < ix + SIZE + 6.0)
        .map(|i| i.2)
}
