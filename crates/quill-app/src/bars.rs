//! The formatting toolbar, for individual elements (font, size, character
//! formats, and the things dropped into text: pictures, tables, borders
//! and graphs), the paragraph toolbar (style, alignment, spacing,
//! lists, columns, the ruler's scales and, if asked for, the tab types) and
//! the ruler (the measuring scale with its indents and tabs).
//!
//! Positions follow the original's on a 640 x 480 screen, in points from the
//! left edge of the window. The buttons are flat, like the top toolbar's, with
//! Quill's own icons on a 16 x 16 grid (see `icon`): a button that is on
//! has a darker rounded backing, the one under the pointer a lighter one.

use crate::canvas::{self, Canvas, Label, Rect, color};
use crate::commands::Cmd;
use crate::icon;
use quill_core::CharProps;
use quill_core::props::{Align, LineSpacing, ParaProps, TabAlign, TabStop, Twips};

/// The formatting and paragraph toolbars use the same 16-point controls as
/// the rest of the interface. Interface Size scales them together. The ruler
/// is not zoomed, so its inches stay in step with the text.
fn zoom() -> f32 {
    1.0
}

/// The look every toolbar and the ruler share, from the settings file.
/// The bars' color (`bar_background`).
pub fn background() -> u32 {
    crate::settings::with(|s| s.bar_background)
}

/// Blank points before the first control and above and below the controls
/// (`bar_padding_horiz` and `bar_padding_vert`).
pub fn padding() -> (f32, f32) {
    crate::settings::with(|s| (s.bar_padding_horiz as f32, s.bar_padding_vert as f32))
}

/// The flat line under a bar that starts at `top`, in the window's own
/// scale (not the bar's zoom), so every divider is the same thickness. It
/// keeps the bar's side padding, like the controls.
fn divider(c: &mut Canvas, top: f32, width: f32) {
    let pad = padding().0;
    let y = top + formatting_toolbar_height() - c.hair();
    c.hline(pad, width - pad, y, line_color());
}

/// `c` a fifth of the way to black.
fn darker(c: u32) -> u32 {
    let ch = |sh: u32| (((c >> sh) & 255) * 4 / 5) << sh;
    ch(16) | ch(8) | ch(0)
}

/// The flat dividers, and the outlines of the white boxes (`line_color`).
pub fn line_color() -> u32 {
    crate::settings::with(|s| s.line_color)
}

/// Heights in the window's interface units, zoom included. Item layouts
/// below are in the original's units, before the zoom.
/// Both toolbars hold 16-point controls, zoomed, with the padding above
/// and below and the divider under them.
pub fn formatting_toolbar_height() -> f32 {
    (16.0 * zoom() + 2.0 * padding().1 + 1.0).round()
}

pub fn paragraph_toolbar_height() -> f32 {
    formatting_toolbar_height()
}

/// The ruler: just the measuring scale.
pub const SCALE_HEIGHT: f32 = 18.0;
const UI: &str = quill_layout::fonts::UI_FONT;

/// A control on the formatting or paragraph toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    FontPopup,
    SizePopup,
    Bold,
    Italic,
    Underline,
    Superscript,
    Subscript,
    Columns(u8),
    StylePopup,
    Align(Align),
    Spacing(u8),
    SpaceBefore(bool),
    TabType(TabAlign),
    Scale(u8),
    /// A button sharing a standard toolbar picture, by its name, and
    /// running that picture's command.
    Tool(&'static str),
}

/// Layout of the formatting toolbar's controls, relative to its top-left.
/// Extra points after each control (`formatting_toolbar_spacing` in the
/// settings file), so the buttons are not shoulder to shoulder as in the original.
/// The original's first control sat 8 points in; this moves the row to the
/// bar's padding instead, and puts the controls below the top padding.
fn spaced(items: impl IntoIterator<Item = (Hit, Rect)>) -> Vec<(Hit, Rect)> {
    let gap = crate::settings::with(|s| s.formatting_toolbar_spacing) as f32;
    let (horiz, vert) = padding();
    let (dx, y) = (horiz / zoom() - 8.0, vert / zoom());
    items
        .into_iter()
        .enumerate()
        .map(|(i, (h, r))| (h, Rect::new(r.x + dx + i as f32 * gap, y, r.w, r.h)))
        .collect()
}

/// Controls left to right from the original's first x, each with its width and
/// the space before it.
fn lay(items: &[(Hit, f32, f32)]) -> Vec<(Hit, Rect)> {
    let mut x = 8.0;
    let placed: Vec<(Hit, Rect)> = items
        .iter()
        .map(|&(hit, w, before)| {
            x += before;
            let r = Rect::new(x, 0.0, w, 16.0);
            x += w;
            (hit, r)
        })
        .collect();
    spaced(placed)
}

/// Extra room between the character formats and the elements.
const SPACER: f32 = 28.0;

/// Font and size, then the character formats, then after a spacer the
/// elements a document holds.
fn formatting_toolbar_items() -> Vec<(Hit, Rect)> {
    lay(&[
        (Hit::FontPopup, 112.0, 0.0),
        (Hit::SizePopup, 52.0, 8.0),
        (Hit::Tool("Up"), 18.0, 8.0),
        (Hit::Tool("Down"), 18.0, 0.0),
        (Hit::Bold, 16.0, 12.0),
        (Hit::Italic, 16.0, 0.0),
        (Hit::Underline, 16.0, 0.0),
        (Hit::Superscript, 18.0, 12.0),
        (Hit::Subscript, 18.0, 0.0),
        (Hit::Tool("Hyperlink"), 18.0, 12.0),
        (Hit::Tool("Remove Link"), 18.0, 0.0),
        (Hit::Tool("Change Case"), 18.0, 12.0),
        (Hit::Tool("Insert Picture"), 18.0, SPACER),
        (Hit::Tool("Insert Table"), 18.0, 0.0),
        (Hit::Tool("Border"), 18.0, 0.0),
        (Hit::Tool("Insert Graph"), 18.0, 0.0),
    ])
}

/// The style, then everything that formats whole paragraphs, then the
/// ruler's scales, and the tab types when `paragraph_toolbar_tabs` is on.
fn paragraph_toolbar_items() -> Vec<(Hit, Rect)> {
    let mut v = vec![(Hit::StylePopup, 112.0, 0.0)];
    for (i, a) in [Align::Left, Align::Center, Align::Right, Align::Justify]
        .into_iter()
        .enumerate()
    {
        v.push((Hit::Align(a), 17.0, if i == 0 { 8.0 } else { 0.0 }));
    }
    for i in 0..3u8 {
        v.push((Hit::Spacing(i), 17.0, if i == 0 { 8.0 } else { 0.0 }));
    }
    v.push((Hit::SpaceBefore(false), 17.0, 8.0));
    v.push((Hit::SpaceBefore(true), 17.0, 0.0));
    v.push((Hit::Tool("Bullet"), 18.0, 12.0));
    v.push((Hit::Tool("Unnest Paragraph"), 18.0, 0.0));
    v.push((Hit::Tool("Nest Paragraph"), 18.0, 0.0));
    for k in 1..=3u8 {
        v.push((Hit::Columns(k), 18.0, if k == 1 { 12.0 } else { 0.0 }));
    }
    for i in 0..3u8 {
        v.push((Hit::Scale(i), 17.0, if i == 0 { 12.0 } else { 0.0 }));
    }
    if crate::settings::with(|s| s.paragraph_toolbar_tabs) {
        for (i, t) in [
            TabAlign::Left,
            TabAlign::Center,
            TabAlign::Right,
            TabAlign::Decimal,
            TabAlign::Bar,
        ]
        .into_iter()
        .enumerate()
        {
            v.push((Hit::TabType(t), 16.0, if i == 0 { 8.0 } else { 0.0 }));
        }
    }
    lay(&v)
}

/// A shared picture's command, if it has one yet.
pub fn tool_command(name: &str) -> Option<Cmd> {
    crate::toolbar::picture_named(name).and_then(|p| p.2)
}

/// A toolbar control's tooltip, with its shortcut if it has one.
pub fn tip(hit: Hit) -> Option<String> {
    let (name, cmd) = match hit {
        Hit::FontPopup => ("Font", None),
        Hit::SizePopup => ("Font Size", None),
        Hit::StylePopup => ("Style", None),
        Hit::Bold => ("Bold", Some(Cmd::Bold)),
        Hit::Italic => ("Italic", Some(Cmd::Italic)),
        Hit::Underline => ("Underline", Some(Cmd::Underline)),
        Hit::Superscript => ("Superscript", Some(Cmd::Superscript)),
        Hit::Subscript => ("Subscript", Some(Cmd::Subscript)),
        Hit::Columns(1) => ("One Column", None),
        Hit::Columns(2) => ("Two Columns", None),
        Hit::Columns(_) => ("Three Columns", None),
        Hit::Align(Align::Left) => ("Align Left", Some(Cmd::AlignLeft)),
        Hit::Align(Align::Center) => ("Centered", Some(Cmd::AlignCenter)),
        Hit::Align(Align::Right) => ("Align Right", Some(Cmd::AlignRight)),
        Hit::Align(Align::Justify) => ("Justified", Some(Cmd::Justify)),
        Hit::Spacing(0) => ("Single Spacing", None),
        Hit::Spacing(1) => ("1.5 Line Spacing", None),
        Hit::Spacing(_) => ("Double Spacing", Some(Cmd::DoubleSpace)),
        Hit::SpaceBefore(false) => ("No Space Before", None),
        Hit::SpaceBefore(true) => ("12 pt Space Before", Some(Cmd::SpaceBefore12)),
        Hit::TabType(TabAlign::Left) => ("Left Tab", None),
        Hit::TabType(TabAlign::Center) => ("Center Tab", None),
        Hit::TabType(TabAlign::Right) => ("Right Tab", None),
        Hit::TabType(TabAlign::Decimal) => ("Decimal Tab", None),
        Hit::TabType(TabAlign::Bar) => ("Vertical Line", None),
        Hit::Scale(0) => ("Normal Scale", None),
        Hit::Scale(1) => ("Table Scale", None),
        Hit::Scale(_) => ("Page Scale", None),
        Hit::Tool(name) => match tool_command(name) {
            Some(c) => (crate::commands::tip_name(c), Some(c)),
            None => (name, None),
        },
    };
    Some(crate::commands::tip(name, cmd))
}

pub fn formatting_toolbar_hit(top: f32, x: f32, y: f32) -> Option<Hit> {
    let (top, x, y) = (top / zoom(), x / zoom(), y / zoom());
    formatting_toolbar_items()
        .into_iter()
        .find(|(_, r)| Rect::new(r.x, r.y + top, r.w, r.h).contains(x, y))
        .map(|(h, _)| h)
}

pub fn paragraph_toolbar_hit(top: f32, x: f32, y: f32) -> Option<Hit> {
    let (top, x, y) = (top / zoom(), x / zoom(), y / zoom());
    paragraph_toolbar_items()
        .into_iter()
        .find(|(_, r)| Rect::new(r.x, r.y + top, r.w, r.h).contains(x, y))
        .map(|(h, _)| h)
}

/// Whether `y` is on the ruler that starts at `top`.
pub fn on_ruler(top: f32, y: f32) -> bool {
    y >= top && y < top + SCALE_HEIGHT
}

/// The popup field rectangle for a toolbar popup, in window coordinates,
/// for placing its menu.
pub fn popup_rect(hit: Hit, formatting_toolbar_top: f32, paragraph_toolbar_top: f32) -> Rect {
    let (formatting_toolbar_top, paragraph_toolbar_top) = (
        formatting_toolbar_top / zoom(),
        paragraph_toolbar_top / zoom(),
    );
    let found = formatting_toolbar_items()
        .into_iter()
        .map(|(h, r)| (h, Rect::new(r.x, r.y + formatting_toolbar_top, r.w, r.h)))
        .chain(
            paragraph_toolbar_items()
                .into_iter()
                .map(|(h, r)| (h, Rect::new(r.x, r.y + paragraph_toolbar_top, r.w, r.h))),
        )
        .find(|(h, _)| *h == hit);
    found
        .map(|(_, r)| Rect::new(r.x * zoom(), r.y * zoom(), r.w * zoom(), r.h * zoom()))
        .unwrap_or_default()
}

/// A field showing a value, with the popup arrow at its right end.
fn popup_field(c: &mut Canvas, label: &mut Label, r: Rect, text: &str, hot: bool) {
    popup_field_font(c, label, r, text, hot, UI);
}

fn popup_field_font(c: &mut Canvas, label: &mut Label, r: Rect, text: &str, hot: bool, font: &str) {
    // The box is outlined in `line_color`, a shade darker under the pointer.
    let line = line_color();
    let edge = if hot { darker(line) } else { line };
    canvas::round_rect(c, r, 3.0, edge);
    canvas::round_rect(c, r.inset(1.0), 2.0, color::WHITE);
    let clip = c.set_clip(Rect::new(r.x + 1.0, r.y + 1.0, r.w - 17.0, r.h - 2.0));
    // `toolbar_picker_font_size` is in points as shown, so undo the zoom
    // the box is drawn at; the baseline centers capitals in the box.
    let size = crate::settings::with(|s| s.toolbar_picker_font_size) as f32 / zoom();
    let base = r.y + r.h / 2.0 + size * 0.36;
    label.draw(c, font, size, false, r.x + 5.0, base, text, icon::INK);
    c.restore_clip(clip);
    let n = c.scale;
    let im = icon::cached(CHEVRON, 9, 6, n);
    icon::paint(
        c,
        label,
        &im,
        r.right() - 13.0,
        r.y + 5.0,
        255,
        color::WHITE,
    );
}

/// A flat button showing `spec`: `on` gets a darker backing, `hot` (under
/// the pointer) a lighter one, and a button that does nothing yet is faded.
#[allow(clippy::too_many_arguments)]
fn button(
    c: &mut Canvas,
    label: &mut Label,
    r: Rect,
    spec: &'static str,
    on: bool,
    hot: bool,
    live: bool,
) {
    if on {
        canvas::round_rect(c, r, 3.0, color::PRESSED);
    } else if hot && live {
        canvas::round_rect(c, r, 3.0, color::HOVER);
    }
    let n = c.scale;
    let im = icon::cached(spec, 16, 16, n);
    let alpha = if live { 255 } else { c.faded() };
    let x = r.x + ((r.w - 16.0) / 2.0).floor();
    icon::paint(c, label, &im, x, r.y, alpha, background());
}

/// A button showing a standard toolbar picture shrunk to the row's 16
/// points, faded if its feature is not built yet.
fn tool_button(c: &mut Canvas, label: &mut Label, r: Rect, name: &str, hot: bool) {
    let Some((spec, (w, h), cmd)) = crate::toolbar::picture_named(name) else {
        return;
    };
    let live = cmd.is_some();
    if hot && live {
        canvas::round_rect(c, r, 3.0, color::HOVER);
    }
    let k = 16.0 / h as f32;
    let old = c.zoom(k);
    let n = c.scale;
    let im = icon::cached(spec, w, h, n);
    let x = r.x + (r.w - w as f32 * k) / 2.0;
    let alpha = if live { 255 } else { c.faded() };
    icon::paint(c, label, &im, x / k, r.y / k, alpha, background());
    c.set_scale(old);
}

/// What the formatting toolbar shows: the selection's formatting.
pub struct FormattingToolbarState<'a> {
    pub font: &'a str,
    pub chp: CharProps,
}

/// Draw the formatting toolbar, with `hover` the control under the pointer.
pub fn draw_formatting_toolbar(
    c: &mut Canvas,
    label: &mut Label,
    top: f32,
    width: f32,
    s: &FormattingToolbarState,
    hover: Option<Hit>,
) {
    let old = c.zoom(zoom());
    let (top, width) = (top / zoom(), width / zoom());
    let bar = Rect::new(0.0, top, width, formatting_toolbar_height() / zoom());
    c.fill(bar, background());
    let at = |r: Rect| Rect::new(r.x, r.y + top, r.w, r.h);
    for (hit, r) in formatting_toolbar_items() {
        let r = at(r);
        let hot = hover == Some(hit);
        let (spec, on, live) = match hit {
            Hit::FontPopup => {
                popup_field_font(c, label, r, s.font, hot, s.font);
                continue;
            }
            Hit::SizePopup => {
                popup_field(c, label, r, &format_size(s.chp.size), hot);
                continue;
            }
            Hit::Bold => (BOLD, s.chp.bold, true),
            Hit::Italic => (ITALIC, s.chp.italic, true),
            Hit::Underline => (
                UNDERLINE,
                s.chp.underline != quill_core::Underline::None,
                true,
            ),
            Hit::Superscript => (SUPERSCRIPT, s.chp.script == quill_core::Script::Super, true),
            Hit::Subscript => (SUBSCRIPT, s.chp.script == quill_core::Script::Sub, true),
            Hit::Tool(name) => {
                tool_button(c, label, r, name, hot);
                continue;
            }
            _ => continue,
        };
        button(c, label, r, spec, on, hot, live);
    }
    c.set_scale(old);
    divider(c, top * zoom(), width * zoom());
}

/// What the paragraph toolbar shows: the paragraph's formatting.
pub struct ParagraphToolbarState<'a> {
    pub style: &'a str,
    pub pap: &'a ParaProps,
    /// The kind of tab a click on the ruler sets.
    pub tab_type: TabAlign,
    /// Columns in the section the insertion point is in.
    pub columns: u16,
}

/// What the ruler shows: the paragraph's indents and tabs.
pub struct RulerState<'a> {
    pub pap: &'a ParaProps,
    /// Where the left margin of the text is on screen.
    pub text_left: f32,
    /// Width of the text column in points.
    pub text_width: f32,
    /// Length of an inch on the ruler, in the units the ruler is drawn in.
    pub inch: f32,
    /// Tabs to show instead of the paragraph's (the Tabs dialog's working
    /// copy), and the one it has chosen, drawn inverted.
    pub tabs: Option<&'a [TabStop]>,
    pub chosen: Option<Twips>,
    /// In a table: the row's cell boundaries, drawn as T markers.
    pub columns: Vec<f32>,
}

/// Draw the paragraph toolbar, with `hover` the control under the pointer.
pub fn draw_paragraph_toolbar(
    c: &mut Canvas,
    label: &mut Label,
    top: f32,
    width: f32,
    s: &ParagraphToolbarState,
    hover: Option<Hit>,
) {
    let old = c.zoom(zoom());
    let (ztop, zwidth) = (top / zoom(), width / zoom());
    let bar = Rect::new(0.0, ztop, zwidth, paragraph_toolbar_height() / zoom());
    c.fill(bar, background());
    let at = |r: Rect| Rect::new(r.x, r.y + ztop, r.w, r.h);
    for (hit, r) in paragraph_toolbar_items() {
        let r = at(r);
        let hot = hover == Some(hit);
        let (spec, on, live) = match hit {
            Hit::StylePopup => {
                popup_field(c, label, r, s.style, hot);
                continue;
            }
            Hit::Align(a) => {
                let spec = match a {
                    Align::Left => ALIGN_LEFT,
                    Align::Center => ALIGN_CENTER,
                    Align::Right => ALIGN_RIGHT,
                    Align::Justify => JUSTIFY,
                };
                (spec, s.pap.align == a, true)
            }
            Hit::Spacing(i) => {
                let on = matches!(
                    (i, s.pap.line),
                    (0, LineSpacing::Auto)
                        | (1, LineSpacing::AtLeast(360))
                        | (2, LineSpacing::AtLeast(480))
                );
                (
                    [SINGLE, ONE_AND_A_HALF, DOUBLE][i.min(2) as usize],
                    on,
                    true,
                )
            }
            Hit::SpaceBefore(open) => (
                if open { OPEN_SPACE } else { CLOSED_SPACE },
                (s.pap.before > 0) == open,
                true,
            ),
            Hit::TabType(t) => {
                let spec = match t {
                    TabAlign::Left => TAB_LEFT,
                    TabAlign::Center => TAB_CENTER,
                    TabAlign::Right => TAB_RIGHT,
                    TabAlign::Decimal => TAB_DECIMAL,
                    TabAlign::Bar => TAB_BAR,
                };
                (spec, s.tab_type == t, true)
            }
            // Only the indent scale is built; the table and page scales
            // come later.
            Hit::Scale(i) => (
                [INDENT_SCALE, TABLE_SCALE, PAGE_SCALE][i.min(2) as usize],
                i == 0,
                i == 0,
            ),
            Hit::Columns(k) => (
                [ONE_COLUMN, TWO_COLUMNS, THREE_COLUMNS][(k as usize).clamp(1, 3) - 1],
                s.columns == k as u16,
                true,
            ),
            Hit::Tool(name) => {
                tool_button(c, label, r, name, hot);
                continue;
            }
            _ => continue,
        };
        button(c, label, r, spec, on, hot, live);
    }
    c.set_scale(old);
    divider(c, top, width);
}

/// Draw the ruler: inches from the left margin, the indent markers and the
/// tab stops.
pub fn draw_ruler(c: &mut Canvas, label: &mut Label, top: f32, width: f32, s: &RulerState) {
    let r = Rect::new(0.0, top, width, SCALE_HEIGHT);
    let (paper, ink) = crate::settings::with(|s| (s.ruler_background, s.ruler_foreground));
    c.fill(r, paper);
    c.hline(0.0, width, r.bottom() - c.hair(), line_color());
    let x0 = s.text_left;
    let base = top + 9.0;
    c.hline(0.0, width, base, ink);
    // Ticks every eighth of an inch, longer at quarters and halves.
    let mut i = 0;
    loop {
        let x = x0 + i as f32 * s.inch / 8.0;
        if x > width {
            break;
        }
        let len = if i % 8 == 0 {
            8.0
        } else if i % 4 == 0 {
            4.0
        } else if i % 2 == 0 {
            2.0
        } else {
            0.0
        };
        if len > 0.0 {
            c.vline(x, base - len, base, ink);
        }
        if i % 8 == 0 {
            let n = (i / 8).to_string();
            label.draw(c, UI, 9.0, false, x + 2.0, top + 8.0, &n, ink);
        }
        i += 1;
    }
    // Table column boundaries: the original's T markers along the top.
    for &x in &s.columns {
        c.fill(Rect::new(x - 3.0, top + 1.0, 7.0, 1.0), ink);
        c.vline(x, top + 1.0, top + 7.0, ink);
    }
    let pt = |t: Twips| x0 + t as f32 / 1440.0 * s.inch;
    let p = s.pap;
    // First line indent hangs from the top; left indent stands on the bottom.
    triangle(c, pt(p.left + p.first), base + 1.0, false, ink);
    triangle(c, pt(p.left), base + 7.0, true, ink);
    let right = pt((s.text_width * 20.0) as Twips - p.right);
    triangle(c, right, base + 7.0, true, ink);
    for t in s.tabs.unwrap_or(&p.tabs) {
        let x = pt(t.pos);
        if s.chosen == Some(t.pos) {
            c.fill(Rect::new(x - 5.0, base, 11.0, 9.0), ink);
            tab_glyph(c, x, base + 8.0, t.align, paper);
        } else {
            tab_glyph(c, x, base + 8.0, t.align, ink);
        }
    }
}

/// A five-point triangle pointing up (standing on `y`) or down (hanging
/// from `y`), centered on `x`.
fn triangle(c: &mut Canvas, x: f32, y: f32, up: bool, col: u32) {
    for i in 0..5 {
        let i = i as f32;
        let w = 2.0 * i + 1.0;
        let yy = if up { y - 5.0 + i } else { y + 4.0 - i };
        c.fill(Rect::new(x - i, yy, w, 1.0), col);
    }
}

/// The small glyph the original draws for each kind of tab stop, standing on `y`.
pub fn tab_glyph(c: &mut Canvas, x: f32, y: f32, kind: TabAlign, col: u32) {
    match kind {
        TabAlign::Left => {
            c.vline(x, y - 7.0, y, col);
            c.hline(x, x + 5.0, y - 1.0, col);
        }
        TabAlign::Right => {
            c.vline(x, y - 7.0, y, col);
            c.hline(x - 4.0, x + 1.0, y - 1.0, col);
        }
        TabAlign::Center => {
            c.vline(x, y - 7.0, y, col);
            c.hline(x - 3.0, x + 4.0, y - 1.0, col);
        }
        TabAlign::Decimal => {
            c.vline(x, y - 7.0, y, col);
            c.hline(x - 3.0, x + 4.0, y - 1.0, col);
            c.fill(Rect::new(x + 2.0, y - 5.0, 1.5, 1.5), col);
        }
        TabAlign::Bar => c.vline(x, y - 8.0, y, col),
    }
}

/// Font size as the formatting toolbar shows it: "12", or "10.5".
pub fn format_size(half_points: u16) -> String {
    if half_points.is_multiple_of(2) {
        (half_points / 2).to_string()
    } else {
        format!("{}.5", half_points / 2)
    }
}

/// What a press on the scale grabbed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScaleGrab {
    FirstIndent,
    /// The left indent marker; the first line indent moves with it.
    LeftIndent,
    RightIndent,
    /// An existing tab stop, by position.
    Tab(Twips),
    /// A new tab of the current type.
    NewTab,
    /// A table column boundary (0 is the row's left edge).
    Column(usize),
}

/// Find what is under a click on the scale.
pub fn scale_grab(s: &RulerState, top: f32, x: f32, y: f32) -> ScaleGrab {
    let x0 = s.text_left;
    let p = s.pap;
    let base = top + 9.0;
    let near = |pos: f32| (x - pos).abs() <= 4.0;
    // T markers sit in the scale's top half.
    if y < base - 3.0
        && let Some(k) = s.columns.iter().position(|&c| near(c))
    {
        return ScaleGrab::Column(k);
    }
    let at = |t: Twips| x0 + t as f32 / 1440.0 * s.inch;
    let first = at(p.left + p.first);
    let left = at(p.left);
    let right = at((s.text_width * 20.0) as Twips - p.right);
    if near(first) && y < base + 2.0 {
        return ScaleGrab::FirstIndent;
    }
    if near(left) && y >= base {
        return ScaleGrab::LeftIndent;
    }
    if near(first) {
        return ScaleGrab::FirstIndent;
    }
    if near(right) {
        return ScaleGrab::RightIndent;
    }
    if let Some(t) = p.tabs.iter().find(|t| near(at(t.pos))) {
        return ScaleGrab::Tab(t.pos);
    }
    ScaleGrab::NewTab
}

/// Twips from the left margin for a pointer at `x`, snapped to the
/// ruler's sixteenth-inch steps as the original does.
pub fn snap(text_left: f32, inch: f32, x: f32) -> Twips {
    let t = ((x - text_left) / inch * 1440.0).round() as Twips;
    let step = 90; // 1/16 inch
    ((t as f32 / step as f32).round() as Twips) * step
}

/// Apply a scale drag to a paragraph format. `start` is the format when the
/// drag began, `at` where the pointer is now (twips from the left margin),
/// and `off` true when it has been pulled off the ruler, which removes a
/// tab.
pub fn apply_grab(
    pap: &mut ParaProps,
    start: &ParaProps,
    grab: ScaleGrab,
    at: Twips,
    kind: TabAlign,
    off: bool,
    text_width: Twips,
) {
    match grab {
        ScaleGrab::FirstIndent => pap.first = at - start.left,
        ScaleGrab::LeftIndent => {
            pap.left = at.max(-1440);
            pap.first = start.first;
        }
        ScaleGrab::RightIndent => pap.right = (text_width - at).max(-1440),
        ScaleGrab::Tab(old) => {
            *pap = start.clone();
            let t = start
                .tabs
                .iter()
                .find(|t| t.pos == old)
                .copied()
                .unwrap_or_default();
            pap.clear_tab(old);
            if !off {
                pap.set_tab(TabStop {
                    pos: at.max(0),
                    ..t
                });
            }
        }
        ScaleGrab::Column(_) => {}
        ScaleGrab::NewTab => {
            *pap = start.clone();
            if !off {
                pap.set_tab(TabStop {
                    pos: at.max(0),
                    align: kind,
                    ..Default::default()
                });
            }
        }
    }
}

// The formatting and paragraph toolbars' icons, on a 16 x 16 grid (the
// chevron on 9 x 6).

const CHEVRON: &str = "
    line k 1 1 4 4 7 1
";
const BOLD: &str = "
    text k Helvetica 12 1 4 12.5 B
";
const ITALIC: &str = "
    line k 7 3 11 3
    line k 5 13 9 13
    line k 9.5 3 6.5 13
";
const UNDERLINE: &str = "
    text k Helvetica 11 0 4 11 U
    line k 4 13 12 13
";
const SUPERSCRIPT: &str = "
    text k Helvetica 10 0 2 13 x
    text B Helvetica 7 1 9 7.5 2
";
const SUBSCRIPT: &str = "
    text k Helvetica 10 0 2 10.5 x
    text B Helvetica 7 1 9 14.5 2
";
const ONE_COLUMN: &str = "
    line k 3 4 13 4
    line k 3 7 13 7
    line k 3 10 13 10
    line k 3 13 13 13
";
const TWO_COLUMNS: &str = "
    line k 2 4 7 4
    line k 2 7 7 7
    line k 2 10 7 10
    line k 2 13 7 13
    line k 9 4 14 4
    line k 9 7 14 7
    line k 9 10 14 10
    line k 9 13 14 13
";
const THREE_COLUMNS: &str = "
    line k 1 4 4 4
    line k 1 7 4 7
    line k 1 10 4 10
    line k 1 13 4 13
    line k 6 4 9 4
    line k 6 7 9 7
    line k 6 10 9 10
    line k 6 13 9 13
    line k 11 4 14 4
    line k 11 7 14 7
    line k 11 10 14 10
    line k 11 13 14 13
";
const ALIGN_LEFT: &str = "
    line k 3 4 13 4
    line k 3 7 10 7
    line k 3 10 13 10
    line k 3 13 9 13
";
const ALIGN_CENTER: &str = "
    line k 3 4 13 4
    line k 5 7 11 7
    line k 3 10 13 10
    line k 6 13 10 13
";
const ALIGN_RIGHT: &str = "
    line k 3 4 13 4
    line k 6 7 13 7
    line k 3 10 13 10
    line k 7 13 13 13
";
const JUSTIFY: &str = "
    line k 3 4 13 4
    line k 3 7 13 7
    line k 3 10 13 10
    line k 3 13 13 13
";
const SINGLE: &str = "
    line k 3 4 13 4
    line k 3 6 13 6
    line k 3 8 13 8
    line k 3 10 13 10
    line k 3 12 13 12
";
const ONE_AND_A_HALF: &str = "
    line k 3 4 13 4
    line k 3 7 13 7
    line k 3 10 13 10
    line k 3 13 13 13
";
const DOUBLE: &str = "
    line k 3 4 13 4
    line k 3 8 13 8
    line k 3 12 13 12
";
const CLOSED_SPACE: &str = "
    line s 3 3 13 3
    line k 3 6 13 6
    line k 3 8 13 8
    line k 3 10 10 10
";
const OPEN_SPACE: &str = "
    line s 3 3 13 3
    line k 3 8 13 8
    line k 3 10 13 10
    line k 3 12 10 12
";
const TAB_LEFT: &str = "
    line k 6 4 6 12 12 12
";
const TAB_CENTER: &str = "
    line k 8 4 8 12
    line k 3 12 13 12
";
const TAB_RIGHT: &str = "
    line k 10 4 10 12 4 12
";
const TAB_DECIMAL: &str = "
    line k 8 4 8 12
    line k 3 12 13 12
    dot k 11 8 0.9
";
const TAB_BAR: &str = "
    line k 8 3 8 13
";
const INDENT_SCALE: &str = "
    line k 2 8 14 8
    poly k - 2.5 3.5 7.5 3.5 5 7
    poly k - 8.5 12.5 13.5 12.5 11 9
";
const TABLE_SCALE: &str = "
    line k 2 4 6 4
    line k 4 4 4 12
    line k 10 4 14 4
    line k 12 4 12 12
";
const PAGE_SCALE: &str = "
    line k 5 3 3 3 3 13 5 13
    line k 11 3 13 3 13 13 11 13
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shared_picture_is_found() {
        crate::settings::update(|s| s.paragraph_toolbar_tabs = true);
        let all = formatting_toolbar_items()
            .into_iter()
            .chain(paragraph_toolbar_items());
        for (hit, _) in all {
            if let Hit::Tool(name) = hit {
                assert!(crate::toolbar::picture_named(name).is_some(), "{name}");
            }
        }
        crate::settings::update(|s| s.paragraph_toolbar_tabs = false);
    }

    #[test]
    fn tab_buttons_only_show_when_asked_for() {
        let tabs = || {
            paragraph_toolbar_items()
                .iter()
                .filter(|(h, _)| matches!(h, Hit::TabType(_)))
                .count()
        };
        crate::settings::update(|s| s.paragraph_toolbar_tabs = false);
        assert_eq!(tabs(), 0);
        crate::settings::update(|s| s.paragraph_toolbar_tabs = true);
        assert_eq!(tabs(), 5);
        crate::settings::update(|s| s.paragraph_toolbar_tabs = false);
    }

    #[test]
    fn every_icon_draws_something() {
        let all = [
            CHEVRON,
            BOLD,
            ITALIC,
            UNDERLINE,
            SUPERSCRIPT,
            SUBSCRIPT,
            ONE_COLUMN,
            TWO_COLUMNS,
            THREE_COLUMNS,
            ALIGN_LEFT,
            ALIGN_CENTER,
            ALIGN_RIGHT,
            JUSTIFY,
            SINGLE,
            ONE_AND_A_HALF,
            DOUBLE,
            CLOSED_SPACE,
            OPEN_SPACE,
            TAB_LEFT,
            TAB_CENTER,
            TAB_RIGHT,
            TAB_DECIMAL,
            TAB_BAR,
            INDENT_SCALE,
            TABLE_SCALE,
            PAGE_SCALE,
        ];
        for spec in all {
            let im = icon::render(spec, 16, 16, 1.0, 0);
            let inked = im.px.iter().filter(|&&p| p >> 24 > 0).count();
            assert!(inked > 3 || !im.texts.is_empty(), "blank icon: {spec}");
        }
    }
}
