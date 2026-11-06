//! Paragraph, table, border, and glyph painting primitives.

use super::*;

/// The annotation mark: a flat speech balloon 21 x 20 points, its tail
/// at the bottom left on the baseline `base`. The writer's initials are
/// drawn over it.
pub(super) fn draw_balloon(c: &mut dyn Surface, x: f32, base: f32) {
    const NOTE: u32 = 0xF6D26B;
    let top = base - 20.0;
    crate::canvas::round_rect(c, Rect::new(x + 2.0, top, 19.0, 16.0), 4.0, NOTE);
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
                Kind::Field(n) => {
                    // An annotation mark: the balloon, the initials in it.
                    let balloon = p.text[g.offset as usize..].starts_with(chars::ANNOTATION);
                    let (size, fy) = if balloon {
                        draw_balloon(c, ox + g.x, y);
                        (quill_layout::layout::ANNOTATION_TEXT, y - 10.0)
                    } else {
                        (st.size, y)
                    };
                    let plain = CharProps {
                        underline: Underline::None,
                        ..*chp
                    };
                    let chp = if balloon { &plain } else { chp };
                    for fg in l.fields.get(n as usize).into_iter().flatten() {
                        // The pane's caption beside the balloon is gray.
                        let caption = balloon && fg.x >= quill_layout::layout::ANNOTATION_WIDTH;
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
pub(super) fn draw_row(
    c: &mut dyn Surface,
    row: &RowGeom,
    ox: f32,
    top: f32,
    height: f32,
    grid: Option<Gridlines>,
) {
    let bottom = top + height;
    for (k, &(_, _, x0, x1)) in row.cells.iter().enumerate() {
        let cell = &row.props.cells[k];
        if cell.shading > 0 {
            let v = (255.0 * (1.0 - cell.shading as f32 / 1000.0)).round() as u32;
            c.fill(
                Rect::new(ox + x0, top, x1 - x0, height),
                v << 16 | v << 8 | v,
            );
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
        let (left, right) = (
            row.cells.first().map_or(0.0, |c| c.2),
            row.cells.last().map_or(0.0, |c| c.3),
        );
        // Each row draws its own top and bottom, so rows of different
        // widths are closed off.
        hline(c, ox + left, ox + right, top);
        hline(c, ox + left, ox + right, bottom);
        vline(c, ox + left);
        for &(_, _, _, x1) in &row.cells {
            vline(c, ox + x1);
        }
    }
    for (k, &(_, _, x0, x1)) in row.cells.iter().enumerate() {
        let b = &row.props.cells[k].borders;
        let k = color::BLACK;
        hline_styled(c, b.top.style, ox + x0, ox + x1, top, k);
        hline_styled(c, b.bottom.style, ox + x0, ox + x1, bottom, k);
        vline_styled(c, b.left.style, ox + x0, top, bottom, k);
        vline_styled(c, b.right.style, ox + x1, top, bottom, k);
    }
}

/// A paragraph's part of a border box: shading, then the lines around it.
pub fn draw_border_box(c: &mut dyn Surface, b: &BorderBox, ox: f32, py: f32) {
    let (l, r) = (ox + b.left, ox + b.right);
    let (t, bt) = (py + b.top, py + b.bottom);
    if b.shading > 0 {
        let v = (255.0 * (1.0 - b.shading as f32 / 1000.0)).round() as u32;
        c.fill(Rect::new(l, t, r - l, bt - t), v << 16 | v << 8 | v);
    }
    let lw = border_width(b.left_line);
    let rw = border_width(b.right_line);
    let k = color::BLACK;
    // Horizontal lines run across the side lines' width too.
    let (hx0, hx1) = (l - lw, r + rw);
    hline_styled(c, b.top_line, hx0, hx1, t - border_width(b.top_line), k);
    let bw = border_width(b.bottom_line);
    hline_styled(c, b.bottom_line, hx0, hx1, bt, k);
    vline_styled(c, b.left_line, l - lw, t, bt, k);
    vline_styled(c, b.right_line, r, t, bt, k);
    if b.shadow {
        // The Shadow preset: a second line outside the right and bottom.
        if b.right_line != BorderStyle::None {
            c.fill(Rect::new(r + rw, t + 2.0, 1.0, bt - t), k);
        }
        if b.bottom_line != BorderStyle::None {
            c.fill(Rect::new(hx0 + 2.0, bt + bw, hx1 - hx0, 1.0), k);
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
        // In dark mode only the desk round the pages goes dark; the pages,
        // and Normal view's text, keep the document's own colors.
        let dark = std::mem::replace(&mut c.dark, false);
        if self.page_view {
            c.dark = dark;
            c.fill(
                area,
                crate::settings::with(|s| s.page_layout_color_background),
            );
            c.dark = false;
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
            c.fill(area, color::WHITE);
            let oy = area.y + TEXT_TOP - self.scroll_y;
            let top = self.scroll_y - TEXT_TOP;
            self.draw_text(c, fonts, cache, focused, ox, oy, top, top + area.h);
            for (first, _) in self.secs.iter().skip(1) {
                // In the middle of the room left for it.
                let y = oy + self.tops[*first] - SECTION_PAD / 2.0;
                if y >= area.y && y <= area.bottom() + 4.0 {
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
        c.dark = dark;
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
            self.draw_text(c, fonts, cache, focused, ox + b.x, oy, from, to);
            if (to - end).abs() < 0.01 && c.screen() {
                c.fill(
                    Rect::new(ox + b.x + 1.0, oy + end + 2.0, 12.0, 1.5),
                    color::BLACK,
                );
            }
            c.pop_clip();
        }
        self.draw_page_notes(c, fonts, cache, k, ox, top + mt);
        self.draw_line_numbers(c, fonts, cache, k, ox, top + mt);
    }

    /// Draw the paragraphs between document y `top` and `bottom`, with
    /// document (0, 0) at window (`ox`, `oy`).
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
    ) {
        let (s, e) = self.sel.range();
        let gridlines = Gridlines::current().filter(|_| c.screen());
        for b in &self.blocks[self.block_at(top)..] {
            if b.top > bottom {
                break;
            }
            if let Some(row) = &b.row {
                draw_row(c, row, ox, oy + b.top, b.height, gridlines);
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
                    draw_border_box(c, bx, px, py);
                }
                let picture = self.selected_picture().is_some();
                if c.screen()
                    && !self.sel.is_empty()
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
        self.draw_frames(c, fonts, cache, focused, ox, oy, top, bottom);
        self.draw_end_notes(c, fonts, cache, ox, oy, top, bottom);
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
        for (li, line) in l.lines.iter().enumerate() {
            let last = li + 1 == l.lines.len();
            let (ls, le) = (line.start, if last { text_len + 1 } else { line.end });
            if to <= ls || from >= le {
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
        if !self.page_view && c.screen() {
            let face = l.glyphs.last().map(|g| g.face).unwrap_or_default();
            for (line, y) in l
                .lines
                .iter()
                .filter(|line| line.hard_break)
                .zip(manual_page_break_rule_ys(l, py))
            {
                // The break can follow text on this very line. Start the
                // guide after that text instead of striking through it.
                let start = ox + line.end_x + 8.0;
                draw_break_rule(
                    c,
                    fonts,
                    cache,
                    face,
                    start.min(ox + self.laid_out_width),
                    ox + self.laid_out_width,
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
