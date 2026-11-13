//! Paragraph, table, border, and glyph painting primitives.

use super::*;

/// The annotation mark: a flat speech balloon 21 x 20 points, its tail
/// at the bottom left on the baseline `base`. The writer's initials are
/// drawn over it.
pub(super) fn draw_balloon(c: &mut dyn Surface, x: f32, base: f32, advance: f32) {
    const NOTE: u32 = 0xF6D26B;
    let top = base - 20.0;
    // Wider than the usual 19 points when the initials and number need it.
    let width = (advance - 4.0).max(19.0);
    crate::canvas::round_rect(c, Rect::new(x + 2.0, top, width, 16.0), 4.0, NOTE);
    // The tail narrows from the balloon's bottom edge to a point.
    let (from, to) = (top + 15.0, base);
    let step = 0.25;
    let mut y = from;
    while y < to {
        let t = (y + step / 2.0 - from) / (to - from);
        let (l, r) = (3.0 - 2.5 * t, 9.0 - 8.5 * t);
        c.fill(Rect::new(x + l, y, (r - l).max(0.0), step), NOTE);
        y += step;
    }
}

/// The shading behind text an annotation is about: the balloon's yellow,
/// paler.
const COMMENT_SHADE: u32 = 0xFCF0C4;
/// Behind text in a font that is not installed.
const MISSING_FONT: u32 = 0xFFD6C8;

/// How far left of the text a change bar is drawn, in points.
const CHANGE_BAR: f32 = 9.0;

#[allow(clippy::too_many_arguments)]
pub fn paint_paragraph(
    c: &mut dyn Surface,
    fonts: &Fonts,
    cache: &mut GlyphCache,
    p: &Paragraph,
    l: &ParaLayout,
    ox: f32,
    py: f32,
    marks: bool,
    _width: f32,
    objects: &[quill_core::Object],
) {
    for (li, line) in l.lines.iter().enumerate() {
        let base = py + line.top + line.baseline;
        // Text an annotation is about is shaded on screen, as the original does.
        if c.screen() {
            for g in l.line_glyphs(line) {
                if l.styles[g.style as usize].chp.comment != 0 && g.advance > 0.0 {
                    c.fill(
                        Rect::new(ox + g.x, py + line.top, g.advance, line.height),
                        COMMENT_SHADE,
                    );
                }
            }
        }
        for g in l.line_glyphs(line) {
            let st = &l.styles[g.style as usize];
            let chp = &st.chp;
            let col = chp.color.0;
            let y = base - st.raise;
            match g.kind {
                Kind::Text | Kind::SoftHyphen if g.advance > 0.0 => {
                    let size = glyph_size(chp, &p.text, g.offset as usize, st.size);
                    draw_styled_glyph(c, fonts, cache, g.face, g.id, size, ox + g.x, y, chp, col);
                }
                Kind::Field => {
                    // An annotation mark: the balloon, the initials in it.
                    let balloon = p.text[g.offset as usize..].starts_with(chars::ANNOTATION);
                    // In the comments pane the mark is only the line saying
                    // who wrote the comment: its icon is drawn with the
                    // comment's box, not as text.
                    let card = balloon && l.card_text > 0.0;
                    let (size, fy) = if card {
                        (l.card_text, y)
                    } else if balloon {
                        draw_balloon(c, ox + g.x, y, g.advance);
                        (quill_layout::layout::ANNOTATION_TEXT, y - 10.0)
                    } else {
                        (st.size, y)
                    };
                    let plain = CharProps {
                        underline: Underline::None,
                        ..*chp
                    };
                    let chp = if balloon { &plain } else { chp };
                    for fg in l.field_glyphs(g.offset) {
                        // The pane's line saying who wrote a comment is gray;
                        // the initials and number in a balloon are not.
                        let caption = card;
                        draw_styled_glyph(
                            c,
                            fonts,
                            cache,
                            fg.face,
                            fg.id,
                            size,
                            ox + g.x + fg.x,
                            fy,
                            chp,
                            if caption { color::GRAY_DARK } else { col },
                        );
                    }
                }
                Kind::Object => match (chp.object as usize)
                    .checked_sub(1)
                    .and_then(|i| objects.get(i))
                {
                    Some(quill_core::Object::Picture(pic)) => {
                        let (w, h) = (pic.size.0 as f32 / 20.0, pic.size.1 as f32 / 20.0);
                        c.picture(pic, Rect::new(ox + g.x, base - h, w, h));
                    }
                    Some(quill_core::Object::Vacant) | None => c.frame(
                        Rect::new(
                            ox + g.x + 1.0,
                            y - st.size * 0.7,
                            g.advance - 2.0,
                            st.size * 0.7,
                        ),
                        col,
                    ),
                },
                Kind::Space if marks => {
                    c.fill(
                        Rect::new(
                            ox + g.x + g.advance / 2.0 - 0.5,
                            y - st.size * 0.3,
                            1.0,
                            1.0,
                        ),
                        col,
                    );
                }
                Kind::Tab(_) if marks => {
                    let mid = ox + g.x + g.advance.min(10.0) / 2.0;
                    let my = y - st.size * 0.3;
                    c.hline(mid - 3.0, mid + 3.0, my, col);
                    c.fill(Rect::new(mid + 1.0, my - 1.0, 1.0, 3.0), col);
                }
                Kind::LineBreak if marks => {
                    // A small return arrow: down from the top, then left.
                    let (x, h) = (ox + g.x + 1.0, st.size * 0.5);
                    c.vline(x + 5.0, y - h, y - 1.0, col);
                    c.hline(x, x + 6.0, y - 1.0, col);
                    c.fill(Rect::new(x + 1.0, y - 3.0, 1.0, 5.0), col);
                }
                _ => {}
            }
        }
        for r in l.rules.iter().filter(|r| r.line as usize == li) {
            let y = base + r.dy;
            match r.kind {
                RuleKind::Solid => c.hline(ox + r.x0, ox + r.x1, y, r.color),
                RuleKind::Double => {
                    c.hline(ox + r.x0, ox + r.x1, y, r.color);
                    c.hline(ox + r.x0, ox + r.x1, y + 2.0, r.color);
                }
                RuleKind::Dotted => c.dotted_hline(ox + r.x0, ox + r.x1, y, r.color),
                RuleKind::Leader(ch) => {
                    // Characters on a fixed step from the margin, so
                    // leaders on different lines line up, with a little
                    // space kept at each end.
                    let id = fonts.glyph_in(r.face, ch);
                    let step = fonts.advance(r.face, id, r.size).max(1.0);
                    let mut x = ((r.x0 + step * 0.5) / step).ceil() * step;
                    while x + step <= r.x1 - step * 0.25 {
                        c.glyph(fonts, cache, r.face, id, r.size, ox + x, y, r.color);
                        x += step;
                    }
                }
            }
        }
        for &bx in &l.bars {
            c.vline(
                ox + bx,
                py + line.top,
                py + line.top + line.height,
                color::BLACK,
            );
        }
        // A tracked change: a bar in the left margin, as the original draws it.
        if line.changed {
            c.vline(
                ox - CHANGE_BAR,
                py + line.top,
                py + line.top + line.height,
                color::BLACK,
            );
        }
    }
    if marks && matches!(p.mark, Mark::CellEnd | Mark::RowEnd) {
        // The original's end-of-cell and end-of-row mark: a small gray dot.
        let last = l.lines.last().unwrap();
        let size = p.mark_chp.size as f32 / 2.0;
        let r = (size / 6.0).max(1.5);
        let cx = ox + last.end_x + r + 1.0;
        let cy = py + last.top + last.baseline - size * 0.35;
        c.fill(
            Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r),
            color::GRAY_DARK,
        );
    } else if marks {
        let last = l.lines.last().unwrap();
        let size = p.mark_chp.size as f32 / 2.0;
        let face = l.glyphs.last().map(|g| g.face).unwrap_or_default();
        let base = py + last.top + last.baseline;
        // A mark that is itself a tracked change, shown, takes its
        // author's color.
        let col = if last.changed && p.mark_chp.rev != 0 {
            quill_layout::layout::shown(p.mark_chp, true).color.0
        } else {
            color::BLACK
        };
        draw_char(
            c,
            fonts,
            cache,
            face,
            '¶',
            size,
            ox + last.end_x + 1.0,
            base,
            col,
        );
    }
}

/// Editing furniture only: a labeled rule never becomes printed content.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_break_rule(
    c: &mut dyn Surface,
    fonts: &Fonts,
    cache: &mut GlyphCache,
    face: quill_layout::FaceKey,
    x0: f32,
    x1: f32,
    y: f32,
    label: &str,
    double: bool,
) {
    if !c.screen() {
        return;
    }
    let size = 9.0;
    let width: f32 = label
        .chars()
        .map(|ch| fonts.advance(face, fonts.glyph_in(face, ch), size))
        .sum();
    let left = (x0 + (x1 - x0 - width) / 2.0).max(x0);
    let right = (left + width).min(x1);
    for dy in if double { &[-1.0, 1.0][..] } else { &[0.0][..] } {
        c.dotted_hline(x0, (left - 5.0).max(x0), y + dy, color::GRAY_DARK);
        c.dotted_hline((right + 5.0).min(x1), x1, y + dy, color::GRAY_DARK);
    }
    c.push_clip(Rect::new(left, y - 6.0, (right - left).max(0.0), 12.0));
    c.fill(
        Rect::new(left, y - 6.0, (right - left).max(0.0), 12.0),
        color::WHITE,
    );
    let mut x = left;
    for ch in label.chars() {
        let id = fonts.glyph_in(face, ch);
        c.glyph(fonts, cache, face, id, size, x, y + 3.0, color::GRAY_DARK);
        x += fonts.advance(face, id, size);
    }
    c.pop_clip();
}

/// The vertical center of the line ending at each manual page break.
/// The visible rule starts after that line's text, never through it.
pub(super) fn manual_page_break_rule_ys(l: &ParaLayout, py: f32) -> impl Iterator<Item = f32> + '_ {
    l.lines
        .iter()
        .filter(|line| line.hard_break)
        .map(move |line| py + line.top + line.height / 2.0)
}

/// Table gridlines as Preferences sets them: shown or not, their color,
/// dotted or solid.
#[derive(Clone, Copy, Debug)]
pub(super) struct Gridlines {
    pub color: u32,
    pub dotted: bool,
}

impl Gridlines {
    /// The gridlines to draw on screen, if they are shown.
    pub(super) fn current() -> Option<Gridlines> {
        crate::settings::with(|s| {
            s.table_gridlines.then_some(Gridlines {
                color: s.table_gridline_color,
                dotted: s.table_gridline_dotted,
            })
        })
    }
}

/// A table row's furniture: cell shading, the on-screen gridlines (when
/// Preferences shows them) and cell borders. The row's top left is at
/// (`ox`, `top`), `ox` being the left margin.
/// Draw a table row's shading and lines. The `selected` cells are filled
/// with `highlight` in place of their shading, under the lines.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_row(
    c: &mut dyn Surface,
    row: &RowGeom,
    ox: f32,
    top: f32,
    height: f32,
    grid: Option<Gridlines>,
    selected: std::ops::RangeInclusive<usize>,
    highlight: u32,
) {
    let bottom = top + height;
    for (k, &(_, _, x0, x1)) in row.cells.iter().enumerate() {
        let cell = &row.props.cells[k];
        let area = Rect::new(ox + x0, top, x1 - x0, height);
        if selected.contains(&k) {
            c.fill(area, highlight);
        } else if let Some(color) = cell.shading_color {
            c.fill(area, color.0);
        } else if cell.shading > 0 {
            let v = (255.0 * (1.0 - cell.shading as f32 / 1000.0)).round() as u32;
            c.fill(area, v << 16 | v << 8 | v);
        }
    }
    if let Some(g) = grid {
        let t = c.pixel();
        let hline = |c: &mut dyn Surface, x0: f32, x1: f32, y: f32| {
            if g.dotted {
                c.dotted_hline(x0, x1, y, g.color);
            } else {
                c.fill(Rect::new(x0, y, x1 - x0 + t, t), g.color);
            }
        };
        let vline = |c: &mut dyn Surface, x: f32| {
            if !g.dotted {
                c.fill(Rect::new(x, top, t, bottom - top), g.color);
                return;
            }
            let mut y = top.floor();
            while y < bottom {
                if (y as i32) % 2 == 0 {
                    c.fill(Rect::new(x, y, 1.0, 1.0), g.color);
                }
                y += 1.0;
            }
        };
        let left = row.cells.first().map_or(0.0, |c| c.2);
        // Each row draws its own top and bottom, so rows of different
        // widths are closed off; none between cells merged down.
        for (k, &(_, _, x0, x1)) in row.cells.iter().enumerate() {
            if !row.joined_above.get(k).copied().unwrap_or(false) {
                hline(c, ox + x0, ox + x1, top);
            }
            if !row.joined_below.get(k).copied().unwrap_or(false) {
                hline(c, ox + x0, ox + x1, bottom);
            }
        }
        vline(c, ox + left);
        for &(_, _, _, x1) in &row.cells {
            vline(c, ox + x1);
        }
    }
    for (k, &(_, _, x0, x1)) in row.cells.iter().enumerate() {
        let b = &row.props.cells[k].borders;
        if !row.joined_above.get(k).copied().unwrap_or(false) {
            hline_styled(c, b.top.style, ox + x0, ox + x1, top, b.top.color.0);
        }
        if !row.joined_below.get(k).copied().unwrap_or(false) {
            hline_styled(
                c,
                b.bottom.style,
                ox + x0,
                ox + x1,
                bottom,
                b.bottom.color.0,
            );
        }
        vline_styled(c, b.left.style, ox + x0, top, bottom, b.left.color.0);
        vline_styled(c, b.right.style, ox + x1, top, bottom, b.right.color.0);
    }
}

/// A paragraph's part of a border box: shading, then the lines around it.
pub fn draw_border_box(c: &mut dyn Surface, b: &BorderBox, ox: f32, py: f32) {
    let (l, r) = (ox + b.left, ox + b.right);
    let (t, bt) = (py + b.top, py + b.bottom);
    if let Some(color) = b.shading_color {
        c.fill(Rect::new(l, t, r - l, bt - t), color.0);
    } else if b.shading > 0 {
        let v = (255.0 * (1.0 - b.shading as f32 / 1000.0)).round() as u32;
        c.fill(Rect::new(l, t, r - l, bt - t), v << 16 | v << 8 | v);
    }
    let lw = border_width(b.left_line);
    let rw = border_width(b.right_line);
    let [top, bottom, left, right] = b.colors.map(|k| k.0);
    // Horizontal lines run across the side lines' width too.
    let (hx0, hx1) = (l - lw, r + rw);
    hline_styled(c, b.top_line, hx0, hx1, t - border_width(b.top_line), top);
    let bw = border_width(b.bottom_line);
    hline_styled(c, b.bottom_line, hx0, hx1, bt, bottom);
    vline_styled(c, b.left_line, l - lw, t, bt, left);
    vline_styled(c, b.right_line, r, t, bt, right);
    if b.shadow {
        // The Shadow preset: a second line outside the right and bottom.
        if b.right_line != BorderStyle::None {
            c.fill(Rect::new(r + rw, t + 2.0, 1.0, bt - t), right);
        }
        if b.bottom_line != BorderStyle::None {
            c.fill(Rect::new(hx0 + 2.0, bt + bw, hx1 - hx0, 1.0), bottom);
        }
    }
}

/// A horizontal border line whose top edge is at `y`.
pub(super) fn hline_styled(
    c: &mut dyn Surface,
    style: BorderStyle,
    x0: f32,
    x1: f32,
    y: f32,
    k: u32,
) {
    let w = border_width(style);
    match style {
        BorderStyle::None => {}
        BorderStyle::Dotted => c.dotted_hline(x0, x1, y, k),
        BorderStyle::Double => {
            c.fill(Rect::new(x0, y, x1 - x0, 1.0), k);
            c.fill(Rect::new(x0, y + 2.0, x1 - x0, 1.0), k);
        }
        _ => c.fill(Rect::new(x0, y, x1 - x0, w.max(c.hair())), k),
    }
}

/// A vertical border line whose left edge is at `x`.
pub(super) fn vline_styled(
    c: &mut dyn Surface,
    style: BorderStyle,
    x: f32,
    y0: f32,
    y1: f32,
    k: u32,
) {
    let w = border_width(style);
    match style {
        BorderStyle::None => {}
        BorderStyle::Dotted => {
            let mut y = y0.floor();
            while y < y1 {
                if (y as i32) % 2 == 0 {
                    c.fill(Rect::new(x, y, 1.0, 1.0), k);
                }
                y += 1.0;
            }
        }
        BorderStyle::Double => {
            c.fill(Rect::new(x, y0, 1.0, y1 - y0), k);
            c.fill(Rect::new(x + 2.0, y0, 1.0, y1 - y0), k);
        }
        _ => c.fill(Rect::new(x, y0, w.max(c.hair()), y1 - y0), k),
    }
}

/// Small caps draw lower-case letters smaller.
pub(super) fn glyph_size(chp: &CharProps, text: &str, offset: usize, size: f32) -> f32 {
    if chp.small_caps
        && !chp.all_caps
        && text[offset..]
            .chars()
            .next()
            .is_some_and(char::is_lowercase)
    {
        size * 0.8
    } else {
        size
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_char(
    c: &mut dyn Surface,
    fonts: &Fonts,
    cache: &mut GlyphCache,
    face: quill_layout::FaceKey,
    ch: char,
    size: f32,
    x: f32,
    y: f32,
    col: u32,
) {
    let id = fonts.glyph_in(face, ch);
    c.glyph(fonts, cache, face, id, size, x, y, col);
}

/// Draw a glyph with QuickDraw's outline and shadow styles when asked.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_styled_glyph(
    c: &mut dyn Surface,
    fonts: &Fonts,
    cache: &mut GlyphCache,
    face: quill_layout::FaceKey,
    id: quill_layout::GlyphId,
    size: f32,
    x: f32,
    y: f32,
    chp: &CharProps,
    col: u32,
) {
    if chp.outline || chp.shadow {
        let d = (size / 18.0).max(0.6);
        let mut offsets = vec![(-d, 0.0), (d, 0.0), (0.0, -d), (0.0, d)];
        if chp.shadow {
            offsets.extend([(2.0 * d, 2.0 * d), (2.0 * d, d), (d, 2.0 * d)]);
        }
        for (dx, dy) in offsets {
            c.glyph_art(fonts, cache, face, id, size, x + dx, y + dy, col);
        }
        c.glyph(fonts, cache, face, id, size, x, y, color::WHITE);
    } else {
        c.glyph(fonts, cache, face, id, size, x, y, col);
    }
}

impl DocView {
    pub fn draw(&mut self, c: &mut Canvas, fonts: &Fonts, cache: &mut GlyphCache, focused: bool) {
        let area = self.area;
        let clip = c.set_clip(area);
        let ox = self.area.x + self.text_x();
        let n = self.layouts.len();
        // In dark mode the desk round the pages takes the interface's dark
        // colors and the pages and their text a dark page's (`page_ink`).
        // Only the drawing changes: the document keeps its own colors.
        let dark_page = std::mem::replace(&mut c.dark_page, c.dark);
        if self.page_view {
            c.dark_page = false;
            c.fill(
                area,
                crate::settings::with(|s| s.page_layout_color_background),
            );
            c.dark_page = c.dark;
            let (_, ph) = self.page_size();
            let left = area.x + self.page_left();
            for k in 0..self.pages.len() {
                let top = area.y - self.scroll_y + self.page_top(k);
                if top > area.bottom() {
                    break;
                }
                if top + ph < area.y {
                    continue;
                }
                self.draw_page(c, fonts, cache, focused, k, left, top);
            }
        } else {
            if self.captions {
                c.fill(area, comment_cards::PANE_BACKGROUND);
                self.draw_comment_cards(c, focused);
            } else {
                c.fill(area, color::WHITE);
            }
            let oy = area.y + TEXT_TOP - self.scroll_y;
            let top = self.scroll_y - TEXT_TOP;
            self.draw_text(c, fonts, cache, focused, ox, oy, top, top + area.h, false);
            let (sel_from, sel_to) = self.sel.range();
            let hl = if focused {
                color::HIGHLIGHT
            } else {
                color::GRAY
            };
            for (first, _) in self.secs.iter().skip(1) {
                // In the middle of the room left for it.
                let y = oy + self.tops[*first] - SECTION_PAD / 2.0;
                if y >= area.y && y <= area.bottom() + 4.0 {
                    // The mark ends the paragraph before it, so a selection
                    // reaching from there into the next one includes it.
                    if !self.sel.is_empty() && sel_from.para < *first && sel_to.para >= *first {
                        c.fill(
                            Rect::new(
                                area.x + 2.0,
                                y - SECTION_PAD / 2.0,
                                area.right() - area.x - 2.0,
                                SECTION_PAD,
                            ),
                            hl,
                        );
                    }
                    self.draw_section_mark(
                        c,
                        fonts,
                        cache,
                        first - 1,
                        area.x + 2.0,
                        area.right(),
                        y,
                    );
                }
            }
            // Automatic page breaks: a row of dots every 4 points across the
            // window. Manual breaks draw their own line.
            let pages = if self.outline {
                &[][..]
            } else {
                &self.pages[..]
            };
            for page in pages.iter().skip(1).filter(|p| !p.manual) {
                let y = (oy + self.pad(page.y)).floor();
                if y < area.y || y > area.bottom() {
                    continue;
                }
                let mut x = area.x + 2.0;
                while x < area.right() {
                    c.fill(Rect::new(x, y, c.hair(), c.hair()), color::BLACK);
                    x += 4.0;
                }
            }
            // The original's end-of-document mark: a short bar under the last line.
            let end_y = oy + self.tops[n] + 2.0;
            c.fill(Rect::new(ox + 1.0, end_y, 12.0, 1.5), color::BLACK);
        }

        if focused && self.is_composing() {
            self.draw_composing(c, fonts, cache);
        } else if focused && self.sel.is_empty() && self.caret_visible() {
            let (x, ty, h) = self.caret_geometry(self.sel.focus, self.upstream);
            let (wx, wy) = self.to_window(x, ty);
            c.fill(Rect::new(wx, wy, c.hair(), h), color::BLACK);
        }
        self.draw_picture_selection(c);
        if focused {
            self.draw_table_handle(c);
        }
        c.dark_page = dark_page;
        c.restore_clip(clip);
    }

    /// Draw page `k` as a framed sheet with its top left corner at
    /// (`left`, `top`): headers, footers, margin page numbers and its share
    /// of the text. Works at any canvas zoom, for Print Preview.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_page(
        &self,
        c: &mut dyn Surface,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        focused: bool,
        k: usize,
        left: f32,
        top: f32,
    ) {
        let (pw, ph) = self.page_size();
        let p = &self.doc.props;
        let mt = self.page_body_top(k);
        let ox = left + (p.margin_left + p.gutter) as f32 / 20.0;
        let n = self.layouts.len();
        let end = self.tops[n];
        let page = Rect::new(left, top, pw, ph);
        if c.screen() {
            c.fill(page, color::WHITE);
            c.frame(
                page.inset(-1.0),
                crate::settings::with(|s| s.page_layout_color_page_border),
            );
        }
        if c.screen() && crate::settings::with(|s| s.text_boundaries) {
            // Preferences > Text Boundaries: the margins, dotted.
            let tr = left + pw - p.margin_right as f32 / 20.0;
            let tb = top + ph - self.page_body_bottom_margin(k);
            c.dotted_hline(ox, tr, top + mt, color::GRAY_DARK);
            c.dotted_hline(ox, tr, tb, color::GRAY_DARK);
            let mut y = (top + mt).floor();
            while y < tb {
                c.fill(Rect::new(ox, y, c.hair(), c.hair()), color::GRAY_DARK);
                c.fill(Rect::new(tr, y, c.hair(), c.hair()), color::GRAY_DARK);
                y += 2.0;
            }
        }
        self.draw_headers(c, fonts, cache, k, ox, top);
        let sec = self.page_section(k);
        if sec.margin_page_numbers {
            // Section > Margin Page Numbers: the number alone, measured
            // from the page's top right corner.
            let (number, format) = self.page_number(k);
            let n = format.text(number);
            let chp = self.doc.styles.get(0).chp;
            let face = fonts.find_resolved(self.doc.font_name(chp.font), false, false);
            let size = chp.size as f32 / 2.0;
            let mut x = left + pw - sec.page_number_from_right as f32 / 20.0;
            let y = top + sec.page_number_from_top as f32 / 20.0 + size;
            for ch in n.chars() {
                let id = fonts.glyph_in(face, ch);
                c.glyph(fonts, cache, face, id, size, x, y, color::BLACK);
                x += fonts.advance(face, id, size);
            }
        }
        // This page's lines: each column's share of the text, shifted onto
        // the page and clipped to its band.
        for b in self.page_bands(k) {
            let (from, to) = (b.y, b.end);
            let band = Rect::new(left, top + mt + b.top, pw, (to - from).max(0.0) + 0.5);
            c.push_clip(band);
            let oy = top + mt + b.top - from;
            self.draw_text(c, fonts, cache, focused, ox + b.x, oy, from, to, true);
            if (to - end).abs() < 0.01 && c.screen() {
                c.fill(
                    Rect::new(ox + b.x + 1.0, oy + end + 2.0, 12.0, 1.5),
                    color::BLACK,
                );
            }
            c.pop_clip();
        }
        self.draw_page_frames(c, fonts, cache, focused, k, ox, top + mt);
        self.draw_page_notes(c, fonts, cache, k, ox, top + mt);
        self.draw_line_numbers(c, fonts, cache, k, ox, top + mt);
    }

    /// Draw the paragraphs between document y `top` and `bottom`, with
    /// document (0, 0) at window (`ox`, `oy`). `on_page` is set when this
    /// is one page's share of the text: boxes are closed off where the page
    /// cuts them, and frames are left to draw_page.
    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &self,
        c: &mut dyn Surface,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        focused: bool,
        ox: f32,
        oy: f32,
        top: f32,
        bottom: f32,
        on_page: bool,
    ) {
        let (s, e) = self.sel.range();
        let gridlines = Gridlines::current().filter(|_| c.screen());
        let cells = self.selected_cells().filter(|_| c.screen());
        let missing = if c.screen() {
            self.missing_font_ids(fonts)
        } else {
            Vec::new()
        };
        let hl = if focused {
            color::HIGHLIGHT
        } else {
            color::GRAY
        };
        // Where a row is drawn: a row carried over pages is closed off
        // where each page cuts it.
        let row_span = |b: &Block| {
            if self.page_view {
                // The closing line goes just inside the page's share,
                // where its clip still shows it.
                let cut = b.top + b.height > bottom + 0.01;
                let end = if cut { bottom - 1.0 } else { b.top + b.height };
                (b.top.max(top), end)
            } else {
                (b.top, b.top + b.height)
            }
        };
        // From the top row of any vertical merge reaching into view: its
        // text, in its top cell, may run down past that row.
        let mut first = self.block_at(top);
        while first > 0
            && self.blocks[first - 1]
                .row
                .as_ref()
                .is_some_and(|r| r.joined_below.iter().any(|&j| j))
        {
            first -= 1;
        }
        // On a page, a block that starts where this share ends belongs to
        // the next page, though its top border would show in the clip.
        let blocks = || {
            self.blocks[first..].iter().take_while(move |b| {
                b.top <= bottom && !(on_page && b.top >= bottom && b.top > top)
            })
        };
        for b in blocks() {
            if let Some(row) = &b.row {
                let (y0, y1) = row_span(b);
                // Selected cells are filled whole, in place of any shading.
                #[allow(clippy::reversed_empty_ranges)]
                let selected = cells
                    .as_ref()
                    .and_then(|block| {
                        let r = block.rows.iter().find(|r| r.first == b.first)?;
                        Some(block.columns(r))
                    })
                    .unwrap_or(1..=0);
                let h = (y1 - y0).max(0.0);
                draw_row(c, row, ox, oy + y0, h, gridlines, selected, hl);
                // Tables in its cells, outer ones first so inner shading
                // and lines go over them.
                for n in &row.nested {
                    #[allow(clippy::reversed_empty_ranges)]
                    draw_row(c, &n.geom, ox, oy + n.top, n.height, gridlines, 1..=0, hl);
                }
            }
            // Outline view may show only a block's first line.
            let clipped =
                self.outline && b.row.is_none() && b.height + 0.01 < self.layout(b.first).height;
            if clipped {
                c.push_clip(Rect::new(-1.0e5, oy + b.top, 2.0e5, b.height));
            }
            for para in b.first..=b.last {
                let l = self.layout(para);
                let (px, py) = (ox + self.xs[para], oy + self.tops[para]);
                if let Some(bx) = &l.border {
                    let bx = if on_page {
                        self.cut_box(para, bx, top, bottom)
                    } else {
                        *bx
                    };
                    draw_border_box(c, &bx, px, py);
                }
                let picture = self.selected_picture().is_some();
                if !missing.is_empty() {
                    self.mark_missing_fonts(c, para, l, px, py, &missing);
                }
                if c.screen()
                    && !self.sel.is_empty()
                    && cells.is_none()
                    && !picture
                    && para >= s.para
                    && para <= e.para
                {
                    self.draw_selection(c, para, l, px, py, s, e, focused);
                }
                self.draw_paragraph(c, fonts, cache, para, l, px, py);
                if self.outline
                    && b.row.is_none()
                    && let Some(line) = l.lines.first()
                {
                    let base = py + line.top + line.baseline;
                    self.draw_symbol(c, para, ox, base, self.xs[para] + line.end_x);
                }
            }
            if clipped {
                c.pop_clip();
            }
        }
        // Each selected cell is outlined just outside its borders, after
        // every row so neighboring rows don't paint over the outlines.
        if let Some(block) = &cells {
            let edge = if focused {
                color::CELL_OUTLINE
            } else {
                color::GRAY_DARK
            };
            let t = c.hair();
            let whole = block.whole_table(&self.doc.main.paras);
            for b in blocks() {
                let Some(row) = &b.row else {
                    continue;
                };
                let Some(r) = block.rows.iter().find(|r| r.first == b.first) else {
                    continue;
                };
                let (y0, y1) = row_span(b);
                // A whole table has one outline, round the outside.
                if whole {
                    let (left, right) = (
                        row.cells.first().map_or(0.0, |c| c.2),
                        row.cells.last().map_or(0.0, |c| c.3),
                    );
                    let (x, w) = (ox + left - t, right - left + 3.0 * t);
                    let (y, h) = (oy + y0 - t, (y1 - y0).max(0.0) + 3.0 * t);
                    c.fill(Rect::new(x, y, t, h), edge);
                    c.fill(Rect::new(x + w - t, y, t, h), edge);
                    if r.first == block.rows[0].first {
                        c.fill(Rect::new(x, y, w, t), edge);
                    }
                    if block.rows.last().is_some_and(|l| l.first == r.first) {
                        c.fill(Rect::new(x, y + h - t, w, t), edge);
                    }
                    continue;
                }
                for k in block.columns(r) {
                    let (_, _, x0, x1) = row.cells[k];
                    // Border lines start on the cell's edges and run right
                    // and down by a hairline.
                    let outside = Rect::new(
                        ox + x0 - t,
                        oy + y0 - t,
                        x1 - x0 + 3.0 * t,
                        (y1 - y0).max(0.0) + 3.0 * t,
                    );
                    c.frame(outside, edge);
                }
            }
        }
        if !on_page {
            self.draw_frames(c, fonts, cache, focused, ox, oy, top, bottom);
        }
        self.draw_end_notes(c, fonts, cache, ox, oy, top, bottom);
    }

    /// Paragraph `para`'s box as a page holding document y `top` to
    /// `bottom` shows it. A box with sides that runs over a page break is
    /// closed off on each page, with its own top and bottom lines, as
    /// table rows are.
    fn cut_box(&self, para: usize, bx: &BorderBox, top: f32, bottom: f32) -> BorderBox {
        let mut bx = *bx;
        if bx.left_line == BorderStyle::None && bx.right_line == BorderStyle::None {
            return bx;
        }
        let paras = &self.doc.main.paras;
        let borders = &paras[para].pap.borders;
        let joins = Joins::around(paras, para);
        let y = self.tops[para];
        let starts_above = y + bx.top < top - 0.01;
        let joined_above = joins.above && y >= top - 0.01 && self.tops[para - 1] < top - 0.01;
        if (starts_above || joined_above) && borders.top.style != BorderStyle::None {
            bx.top_line = borders.top.style;
            bx.colors[0] = borders.top.color;
            bx.top = bx.top.max(top - y) + border_width(bx.top_line);
        }
        let ends_below = y + bx.bottom > bottom + 0.01;
        let joined_below = joins.below && self.tops[para + 1] >= bottom - 0.01;
        if (ends_below || joined_below) && borders.bottom.style != BorderStyle::None {
            bx.bottom_line = borders.bottom.style;
            bx.colors[1] = borders.bottom.color;
            // Just inside the page's share, where its clip still shows it.
            bx.bottom = bx.bottom.min(bottom - y) - border_width(bx.bottom_line);
        }
        bx
    }

    /// How wide paragraph `para` was laid out: its column, or its table
    /// cell, which is narrower than the text area.
    fn para_width(&self, para: usize) -> f32 {
        self.widths
            .get(para)
            .copied()
            .filter(|&w| w > 0.0)
            .unwrap_or(self.laid_out_width)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_selection(
        &self,
        c: &mut dyn Surface,
        para: usize,
        l: &ParaLayout,
        ox: f32,
        py: f32,
        s: Position,
        e: Position,
        focused: bool,
    ) {
        let text_len = self.doc.main.paras[para].len();
        let from = if para == s.para { s.offset } else { 0 };
        let to = if para == e.para {
            e.offset
        } else {
            text_len + 1
        };
        let hl = if focused {
            color::HIGHLIGHT
        } else {
            color::GRAY
        };
        Self::fill_chars(c, l, ox, py, from, to, text_len, self.para_width(para), hl);
    }

    /// The font ids to mark on screen: those not installed, when View >
    /// Highlight Missing Fonts is on.
    fn missing_font_ids(&self, fonts: &Fonts) -> Vec<u16> {
        if !crate::settings::with(|s| s.highlight_missing_fonts) {
            return Vec::new();
        }
        (0..self.doc.fonts.len())
            .filter_map(|id| u16::try_from(id).ok())
            .filter(|&id| fonts.substitute(self.doc.font_name(id)).is_some())
            .collect()
    }

    /// Mark the text of paragraph `para` set in one of the fonts `missing`.
    fn mark_missing_fonts(
        &self,
        c: &mut dyn Surface,
        para: usize,
        l: &ParaLayout,
        ox: f32,
        py: f32,
        missing: &[u16],
    ) {
        let p = &self.doc.main.paras[para];
        for (from, to, chp) in p.run_bounds() {
            if missing.contains(&chp.font) {
                Self::fill_chars(
                    c,
                    l,
                    ox,
                    py,
                    from,
                    to,
                    p.len(),
                    self.para_width(para),
                    MISSING_FONT,
                );
            }
        }
    }

    /// Fill behind the characters `from..to`, line by line.
    #[allow(clippy::too_many_arguments)]
    fn fill_chars(
        c: &mut dyn Surface,
        l: &ParaLayout,
        ox: f32,
        py: f32,
        from: usize,
        to: usize,
        text_len: usize,
        width: f32,
        hl: u32,
    ) {
        for (li, line) in l.lines.iter().enumerate() {
            let last = li + 1 == l.lines.len();
            let (ls, le) = (line.start, if last { text_len + 1 } else { line.end });
            if to <= ls || from >= le {
                continue;
            }
            // A page break has a line to itself and no width of its own:
            // when it is selected, the whole line shows it.
            if line.hard_break && line.end - line.start == 1 {
                if from <= ls && to > ls {
                    // To the right edge of the paragraph's own column or
                    // cell, and no further.
                    let w = (width - line.left).max(0.0);
                    c.fill(Rect::new(ox + line.left, py + line.top, w, line.height), hl);
                }
                continue;
            }
            let x0 = if from > ls {
                l.caret(from, false).1
            } else {
                line.left
            };
            let x1 = if to < le {
                l.caret(to, true).1
            } else if last && to > text_len {
                line.end_x + 6.0 // the paragraph mark
            } else {
                line.end_x
            };
            if x1 > x0 {
                c.fill(Rect::new(ox + x0, py + line.top, x1 - x0, line.height), hl);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_paragraph(
        &self,
        c: &mut dyn Surface,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        para: usize,
        l: &ParaLayout,
        ox: f32,
        py: f32,
    ) {
        paint_paragraph(
            c,
            fonts,
            cache,
            &self.doc.main.paras[para],
            l,
            ox,
            py,
            self.show_marks,
            self.laid_out_width,
            &self.doc.objects,
        );
        self.draw_spelling(c, para, l, ox, py);
        if !self.page_view && c.screen() {
            let face = l.glyphs.last().map(|g| g.face).unwrap_or_default();
            for y in manual_page_break_rule_ys(l, py) {
                // The break has its own line, so the guide runs the width
                // of the text area, as a section break's does.
                draw_break_rule(
                    c,
                    fonts,
                    cache,
                    face,
                    ox,
                    ox + self.para_width(para),
                    y,
                    "Page Break",
                    false,
                );
            }
        }
    }

    fn caret_visible(&self) -> bool {
        let t = self.caret_since.elapsed().as_millis() / BLINK.as_millis();
        t.is_multiple_of(2)
    }

    /// When the caret next turns on or off.
    pub fn next_blink(&self) -> Instant {
        let since = self.caret_since.elapsed().as_millis();
        let period = BLINK.as_millis();
        let next = (since / period + 1) * period;
        self.caret_since + Duration::from_millis(next as u64)
    }

    pub(super) fn reset_blink(&mut self) {
        self.caret_since = Instant::now();
    }
}

#[cfg(test)]
mod break_tests {
    use super::*;

    struct PrintSurface;
    impl Surface for PrintSurface {
        fn screen(&self) -> bool {
            false
        }
        fn hair(&self) -> f32 {
            1.0
        }
        fn fill(&mut self, _: Rect, _: u32) {
            panic!("editing guide leaked into print output");
        }
        fn glyph(
            &mut self,
            _: &Fonts,
            _: &mut GlyphCache,
            _: quill_layout::FaceKey,
            _: quill_layout::GlyphId,
            _: f32,
            _: f32,
            _: f32,
            _: u32,
        ) {
            panic!("editing label leaked into print output");
        }
        fn push_clip(&mut self, _: Rect) {
            panic!("editing clip leaked into print output");
        }
        fn pop_clip(&mut self) {
            panic!("editing clip leaked into print output");
        }
        fn picture(&mut self, _: &quill_core::Picture, _: Rect) {
            panic!("unexpected picture");
        }
    }

    #[test]
    fn break_guides_never_draw_on_print_surfaces() {
        let fonts = Fonts::new(quill_layout::Catalog::from_files(&[]));
        for label in ["Page Break", "Section Break (Continuous)"] {
            draw_break_rule(
                &mut PrintSurface,
                &fonts,
                &mut GlyphCache::new(),
                Default::default(),
                0.0,
                400.0,
                20.0,
                label,
                false,
            );
        }
    }
}

#[cfg(test)]
mod page_cut_tests {
    use super::*;
    use quill_core::props::{BorderLine, Frame, FrameH, FrameRel, FrameV};

    /// Records the fills and glyphs a page shows: those inside its clips.
    #[derive(Default)]
    struct Shown {
        clips: Vec<Rect>,
        fills: Vec<Rect>,
        glyphs: Vec<(f32, f32)>,
    }

    impl Shown {
        fn visible(&self, x: f32, y: f32) -> bool {
            self.clips
                .iter()
                .all(|c| x >= c.x && x <= c.right() && y >= c.y && y <= c.bottom())
        }
        fn rules(&self) -> Vec<Rect> {
            self.fills.iter().copied().filter(|r| r.w > r.h).collect()
        }
    }

    impl Surface for Shown {
        fn screen(&self) -> bool {
            false
        }
        fn hair(&self) -> f32 {
            0.5
        }
        fn fill(&mut self, r: Rect, _: u32) {
            if self.visible(r.x + r.w / 2.0, r.y + r.h / 2.0) {
                self.fills.push(r);
            }
        }
        fn glyph(
            &mut self,
            _: &Fonts,
            _: &mut GlyphCache,
            _: quill_layout::FaceKey,
            _: quill_layout::GlyphId,
            _: f32,
            x: f32,
            baseline: f32,
            _: u32,
        ) {
            if self.visible(x, baseline) {
                self.glyphs.push((x, baseline));
            }
        }
        fn push_clip(&mut self, r: Rect) {
            self.clips.push(r);
        }
        fn pop_clip(&mut self) {
            self.clips.pop();
        }
        fn picture(&mut self, _: &quill_core::Picture, _: Rect) {}
    }

    const FILLER: &str =
        "Filler text that takes up room on the page so the next thing lands near the bottom. ";

    /// Seven paragraphs of filler, then `rest`, laid out in Page Layout on
    /// a page that leaves `room` points after the filler.
    fn view_with(rest: &[String], room: f32, fonts: &mut Fonts) -> DocView {
        let mut text: Vec<String> = (0..7).map(|_| FILLER.repeat(6)).collect();
        text.extend(rest.iter().cloned());
        text.push("After.".into());
        let mut doc = quill_core::text::from_plain_text(&text.join("\n"));
        for p in &mut doc.main.paras[..7] {
            p.pap.after = 200;
        }
        let mut view = DocView::new(doc, "Cut".into());
        view.page_view = true;
        view.area = Rect::new(0.0, 0.0, 900.0, 700.0);
        view.update_layout(fonts);
        let p = &mut view.doc.props;
        p.page_height = p.margin_top + p.margin_bottom + ((view.tops[7] + room) * 20.0) as i32;
        view.update_layout(fonts);
        view
    }

    /// Lay out again after changing paragraph properties.
    fn relayout(view: &mut DocView, fonts: &mut Fonts) {
        view.layouts.clear();
        view.update_layout(fonts);
    }

    fn page(view: &DocView, fonts: &Fonts, k: usize) -> Shown {
        let mut s = Shown::default();
        view.draw_page(&mut s, fonts, &mut GlyphCache::new(), false, k, 0.0, 0.0);
        s
    }

    fn line() -> BorderLine {
        BorderLine {
            style: BorderStyle::Single,
            space: 80,
            ..Default::default()
        }
    }

    #[test]
    fn a_box_over_a_page_break_is_closed_on_both_pages() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let lines: Vec<String> = (1..=8).map(|k| format!("Line {k} in the box.")).collect();
        let mut view = view_with(&lines, 60.0, &mut fonts);
        for p in &mut view.doc.main.paras[7..15] {
            p.pap.borders.top = line();
            p.pap.borders.bottom = line();
            p.pap.borders.left = line();
            p.pap.borders.right = line();
        }
        relayout(&mut view, &mut fonts);
        assert_eq!(view.pages.len(), 2);
        let first = view.pages[1].para;
        assert!(first > 7 && first < 15, "the box runs over the break");
        // Each page shows a top and a bottom edge across the box.
        for k in 0..2 {
            assert_eq!(page(&view, &fonts, k).rules().len(), 2, "page {}", k + 1);
        }
    }

    #[test]
    fn a_ruled_paragraph_moved_to_the_next_page_leaves_no_rule_behind() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let rest = vec!["A ruled paragraph. ".repeat(12)];
        let mut view = view_with(&rest, 30.0, &mut fonts);
        let ruled = &mut view.doc.main.paras[7].pap;
        ruled.keep_together = true;
        ruled.before = 200;
        ruled.borders.top = line();
        ruled.borders.bottom = line();
        relayout(&mut view, &mut fonts);
        assert_eq!(view.pages[1].para, 7, "the ruled paragraph starts page 2");
        assert!(page(&view, &fonts, 0).rules().is_empty());
        assert_eq!(page(&view, &fonts, 1).rules().len(), 2);
    }

    #[test]
    fn a_frame_from_its_paragraph_moves_whole_to_the_next_page() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let rest = vec!["A framed paragraph. ".repeat(20), FILLER.repeat(6)];
        let mut view = view_with(&rest, 30.0, &mut fonts);
        view.doc.main.paras[7].pap.frame = Some(Frame {
            h: FrameH::Right,
            v: FrameV::At(0),
            v_rel: FrameRel::Column,
            width: Some(2880),
            ..Default::default()
        });
        relayout(&mut view, &mut fonts);
        let f = view.frames[0];
        // Placed where its paragraph is, which is too near the foot of
        // page 1 for it, so at the top of page 2.
        assert_eq!(view.on_page(f.y).0, 1);
        assert_eq!(view.on_page(f.y).2, 0.0);
        // Every line of it shows on page 2, though page 2 holds little text.
        let lines = view.layout(7).lines.len();
        let shown = page(&view, &fonts, 1);
        let rows: std::collections::BTreeSet<i32> = shown
            .glyphs
            .iter()
            .filter(|g| g.0 > 300.0)
            .map(|g| g.1.round() as i32)
            .collect();
        assert_eq!(rows.len(), lines);
    }
}
