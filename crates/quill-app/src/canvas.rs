//! A tiny QuickDraw: rectangles, lines, 8x8 patterns and anti-aliased
//! glyphs drawn straight into the window's pixel buffer.
//!
//! Callers work in points (the original's 72 dpi pixels); the canvas
//! multiplies by the display's scale factor. Colors are 0x00RRGGBB.

use quill_layout::fonts::FaceKey;
use quill_layout::{Fonts, GlyphCache, Mask};

/// A rectangle in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }
}

/// Device-pixel rectangle, half-open.
#[derive(Clone, Copy, Debug)]
struct Px {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

pub mod color {
    pub const WHITE: u32 = 0xFFFFFF;
    pub const BLACK: u32 = 0x000000;
    /// Neutral shared control surfaces.
    pub const GRAY: u32 = 0xDDDDDD;
    pub const GRAY_DARK: u32 = 0x888888;
    /// The selection highlight seen in the reference setup.
    pub const HIGHLIGHT: u32 = 0xCCCCFF;
    /// Flat buttons: the backing under the pointer, and the one held down
    /// or switched on.
    pub const HOVER: u32 = 0xCACACA;
    pub const PRESSED: u32 = 0xB2B2B2;
}

/// Color `c` as dark mode shows the interface: grays turn over, so white
/// panels go dark and black labels go light; pale tints darken; strong
/// colors (icons, the accent blue) stay as they are.
pub fn dark_ink(c: u32) -> u32 {
    let (r, g, b) = ((c >> 16) & 255, (c >> 8) & 255, c & 255);
    let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
    if hi - lo < 32 {
        let v = (r + g + b) / 3;
        let d = 0x1E + (255 - v) * (0xE6 - 0x1E) / 255;
        return d << 16 | d << 8 | d;
    }
    if lo > 0xB0 {
        let k = |x: u32| x * 2 / 5;
        return k(r) << 16 | k(g) << 8 | k(b);
    }
    c
}

/// Coverage-to-alpha curve that darkens light anti-aliasing a little, so
/// black text on white does not look washed out without gamma-correct
/// blending.
fn coverage_lut() -> &'static [u8; 256] {
    use std::sync::OnceLock;
    static LUT: OnceLock<[u8; 256]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = [0u8; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = ((i as f32 / 255.0).powf(0.8) * 255.0).round() as u8;
        }
        t
    })
}

pub struct Canvas<'a> {
    buf: &'a mut [u32],
    width: i32,
    height: i32,
    pub scale: f32,
    /// Suppress selections, break guides, and page furniture for printer output.
    pub printing: bool,
    clip: Px,
    /// Clips saved by `push_clip`.
    clips: Vec<Px>,
    /// Draw in dark mode's colors (see `dark_ink`). Off while the page
    /// and its text are drawn, which keep their own colors.
    pub dark: bool,
}

impl<'a> Canvas<'a> {
    pub fn new(buf: &'a mut [u32], width: u32, height: u32, scale: f32) -> Self {
        let (w, h) = (width as i32, height as i32);
        Canvas {
            buf,
            width: w,
            height: h,
            scale,
            printing: false,
            clip: Px {
                x0: 0,
                y0: 0,
                x1: w,
                y1: h,
            },
            clips: Vec::new(),
            dark: false,
        }
    }

    /// The color to draw `c` in: itself, or its dark mode color.
    fn ink(&self, c: u32) -> u32 {
        if self.dark { dark_ink(c) } else { c }
    }

    /// Size of the canvas in points.
    pub fn size(&self) -> (f32, f32) {
        (
            self.width as f32 / self.scale,
            self.height as f32 / self.scale,
        )
    }

    fn px(&self, r: Rect) -> Px {
        let s = self.scale;
        Px {
            x0: (r.x * s).round() as i32,
            y0: (r.y * s).round() as i32,
            x1: (r.right() * s).round() as i32,
            y1: (r.bottom() * s).round() as i32,
        }
    }

    fn clipped(&self, p: Px) -> Px {
        Px {
            x0: p.x0.max(self.clip.x0),
            y0: p.y0.max(self.clip.y0),
            x1: p.x1.min(self.clip.x1),
            y1: p.y1.min(self.clip.y1),
        }
    }

    /// Restrict drawing to `r` (within the current clip). Returns the
    /// previous clip so it can be restored.
    pub fn set_clip(&mut self, r: Rect) -> ClipToken {
        let old = self.clip;
        let p = self.px(r);
        // Clips nest: drawing stays inside the one already set too.
        self.clip = Px {
            x0: p.x0.max(old.x0),
            y0: p.y0.max(old.y0),
            x1: p.x1.min(old.x1),
            y1: p.y1.min(old.y1),
        };
        ClipToken(old)
    }

    pub fn restore_clip(&mut self, t: ClipToken) {
        self.clip = t.0;
    }

    /// Draw at `k` times the size from now on (for the window furniture,
    /// which is scaled up from the original's 72 dpi sizes). Returns the old
    /// scale for `set_scale`.
    pub fn zoom(&mut self, k: f32) -> f32 {
        let old = self.scale;
        self.scale *= k;
        old
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
    }

    /// Width of a one-point line in device pixels, at least 1.
    pub fn hair(&self) -> f32 {
        (self.scale.round().max(1.0)) / self.scale
    }

    pub fn fill(&mut self, r: Rect, color: u32) {
        let color = self.ink(color);
        let p = self.clipped(self.px(r));
        if p.x0 >= p.x1 {
            return;
        }
        for y in p.y0..p.y1 {
            let row = (y * self.width) as usize;
            self.buf[row + p.x0 as usize..row + p.x1 as usize].fill(color);
        }
    }

    /// A diffuse current-macOS panel shadow. The shadow is evaluated as a
    /// rounded-rectangle distance field, giving a soft halo without storing
    /// or shipping a bitmap asset. It is slightly lower than the panel, as
    /// in AppKit's menus and sheets.
    pub fn panel_shadow(&mut self, r: Rect, radius: f32) {
        let sigma = 5.0_f32;
        let reach = 15.0_f32;
        let shadow = Rect::new(r.x, r.y + 3.0, r.w, r.h);
        let bounds = Rect::new(
            shadow.x - reach,
            shadow.y - reach,
            shadow.w + 2.0 * reach,
            shadow.h + 2.0 * reach,
        );
        let p = self.clipped(self.px(bounds));
        let s = self.scale;
        let cx = (shadow.x + shadow.w / 2.0) * s;
        let cy = (shadow.y + shadow.h / 2.0) * s;
        let panel_cy = (r.y + r.h / 2.0) * s;
        let hx = shadow.w * s / 2.0;
        let hy = shadow.h * s / 2.0;
        let rad = radius.min(shadow.w / 2.0).min(shadow.h / 2.0) * s;
        let sigma = sigma * s;
        let reach = reach * s;
        for y in p.y0..p.y1 {
            for x in p.x0..p.x1 {
                let qx = (x as f32 + 0.5 - cx).abs() - (hx - rad);
                let qy = (y as f32 + 0.5 - cy).abs() - (hy - rad);
                let outside = qx.max(0.0).hypot(qy.max(0.0));
                let distance = outside + qx.max(qy).min(0.0) - rad;
                // Skip only the original panel, not the interior of the
                // downward-offset shadow. Skipping the latter leaves a
                // conspicuous clear shelf below the panel on Retina screens.
                let panel_qy = (y as f32 + 0.5 - panel_cy).abs() - (hy - rad);
                let panel_outside = qx.max(0.0).hypot(panel_qy.max(0.0));
                let panel_distance = panel_outside + qx.max(panel_qy).min(0.0) - rad;
                if panel_distance <= 0.0 || distance > reach {
                    continue;
                }
                let alpha =
                    (48.0 * (-distance * distance / (2.0 * sigma * sigma)).exp()).round() as u32;
                if alpha == 0 {
                    continue;
                }
                let pixel = &mut self.buf[(y * self.width + x) as usize];
                let (pr, pg, pb) = ((*pixel >> 16) & 255, (*pixel >> 8) & 255, *pixel & 255);
                let dim = |v: u32| (v * (255 - alpha) + 127) / 255;
                *pixel = (dim(pr) << 16) | (dim(pg) << 8) | dim(pb);
            }
        }
    }

    /// One-point frame inside `r`.
    pub fn frame(&mut self, r: Rect, color: u32) {
        let t = self.hair();
        self.fill(Rect::new(r.x, r.y, r.w, t), color);
        self.fill(Rect::new(r.x, r.bottom() - t, r.w, t), color);
        self.fill(Rect::new(r.x, r.y, t, r.h), color);
        self.fill(Rect::new(r.right() - t, r.y, t, r.h), color);
    }

    pub fn hline(&mut self, x0: f32, x1: f32, y: f32, color: u32) {
        let t = self.hair();
        self.fill(Rect::new(x0, y, x1 - x0, t), color);
    }

    pub fn vline(&mut self, x: f32, y0: f32, y1: f32, color: u32) {
        let t = self.hair();
        self.fill(Rect::new(x, y0, t, y1 - y0), color);
    }

    /// A dotted horizontal line, one point on and one off, like QuickDraw's
    /// gray pen pattern.
    pub fn dotted_hline(&mut self, x0: f32, x1: f32, y: f32, color: u32) {
        let mut x = x0.floor();
        while x < x1 {
            if (x as i32) % 2 == 0 {
                self.fill(Rect::new(x, y, 1.0, self.hair()), color);
            }
            x += 1.0;
        }
    }

    /// A filled, anti-aliased circle centered on (`cx`, `cy`), radius `r`, in
    /// points.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: u32) {
        let color = self.ink(color);
        let s = self.scale;
        let (pcx, pcy, pr) = (cx * s, cy * s, r * s);
        let x0 = ((pcx - pr).floor() as i32).max(self.clip.x0);
        let x1 = ((pcx + pr).ceil() as i32).min(self.clip.x1);
        let y0 = ((pcy - pr).floor() as i32).max(self.clip.y0);
        let y1 = ((pcy + pr).ceil() as i32).min(self.clip.y1);
        let (fr, fg, fb) = ((color >> 16) & 255, (color >> 8) & 255, color & 255);
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = x as f32 + 0.5 - pcx;
                let dy = y as f32 + 0.5 - pcy;
                let a = (pr - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let a = (a * 255.0) as u32;
                let d = &mut self.buf[(y * self.width + x) as usize];
                let (br, bg, bb) = ((*d >> 16) & 255, (*d >> 8) & 255, *d & 255);
                let mix = |f: u32, b: u32| (b * (255 - a) + f * a + 127) / 255;
                *d = (mix(fr, br) << 16) | (mix(fg, bg) << 8) | mix(fb, bb);
            }
        }
    }

    /// Blend a glyph mask with its origin at device pixel (`ox`, `oy`).
    pub fn mask(&mut self, m: &Mask, ox: i32, oy: i32, color: u32) {
        let color = self.ink(color);
        let lut = coverage_lut();
        let x0 = ox + m.left;
        let y0 = oy + m.top;
        let (fr, fg, fb) = ((color >> 16) & 255, (color >> 8) & 255, color & 255);
        for my in 0..m.height as i32 {
            let y = y0 + my;
            if y < self.clip.y0 || y >= self.clip.y1 {
                continue;
            }
            let row = (y * self.width) as usize;
            let src = (my as u32 * m.width) as usize;
            for mx in 0..m.width as i32 {
                let x = x0 + mx;
                if x < self.clip.x0 || x >= self.clip.x1 {
                    continue;
                }
                let a = lut[m.data[src + mx as usize] as usize] as u32;
                if a == 0 {
                    continue;
                }
                let d = &mut self.buf[row + x as usize];
                if a == 255 {
                    *d = color;
                    continue;
                }
                let (br, bg, bb) = ((*d >> 16) & 255, (*d >> 8) & 255, *d & 255);
                let mix = |f: u32, b: u32| (b * (255 - a) + f * a + 127) / 255;
                *d = (mix(fr, br) << 16) | (mix(fg, bg) << 8) | mix(fb, bb);
            }
        }
    }

    /// Blend premultiplied 0xAARRGGBB pixels, `w` wide, with their top left
    /// at device pixel (`ox`, `oy`), faded to `alpha` (0-255).
    pub fn image(&mut self, px: &[u32], w: u32, ox: i32, oy: i32, alpha: u32) {
        let h = px.len() as i32 / w.max(1) as i32;
        for iy in 0..h {
            let y = oy + iy;
            if y < self.clip.y0 || y >= self.clip.y1 {
                continue;
            }
            for ix in 0..w as i32 {
                let x = ox + ix;
                if x < self.clip.x0 || x >= self.clip.x1 {
                    continue;
                }
                let mut s = px[(iy * w as i32 + ix) as usize];
                let sa = (s >> 24) * alpha / 255;
                if sa == 0 {
                    continue;
                }
                if self.dark {
                    // Unpremultiply, take the dark color, premultiply again.
                    let a = (s >> 24).max(1);
                    let un = |sh: u32| (((s >> sh) & 255) * 255 / a).min(255);
                    let c = dark_ink(un(16) << 16 | un(8) << 8 | un(0));
                    let pre = |sh: u32| ((c >> sh) & 255) * a / 255;
                    s = a << 24 | pre(16) << 16 | pre(8) << 8 | pre(0);
                }
                let d = &mut self.buf[(y * self.width + x) as usize];
                let ch = |sh: u32| {
                    let sc = ((s >> sh) & 255) * alpha / 255;
                    let dc = (*d >> sh) & 255;
                    (sc + (dc * (255 - sa) + 127) / 255).min(255)
                };
                *d = (ch(16) << 16) | (ch(8) << 8) | ch(0);
            }
        }
    }

    /// Draw decoded picture pixels into `dst` (points). `src` is the part
    /// of the image shown, as fractions of its width and height (left, top,
    /// right, bottom); beyond 0..1 is white space. Smoothed by bilinear
    /// sampling.
    pub fn picture(&mut self, d: &crate::image::Decoded, src: [f32; 4], dst: Rect) {
        let p = self.px(dst);
        let c = self.clipped(p);
        let (pw, ph) = ((p.x1 - p.x0).max(1) as f32, (p.y1 - p.y0).max(1) as f32);
        let (w, h) = (d.w as i32, d.h as i32);
        let at = |x: i32, y: i32| d.px[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize];
        for y in c.y0..c.y1 {
            let fy = src[1] + (y - p.y0) as f32 / ph * (src[3] - src[1]);
            if !(0.0..1.0).contains(&fy) {
                continue;
            }
            let v = fy * h as f32 - 0.5;
            let (y0, ty) = (v.floor() as i32, v - v.floor());
            for x in c.x0..c.x1 {
                let fx = src[0] + (x - p.x0) as f32 / pw * (src[2] - src[0]);
                if !(0.0..1.0).contains(&fx) {
                    continue;
                }
                let u = fx * w as f32 - 0.5;
                let (x0, tx) = (u.floor() as i32, u - u.floor());
                let (a, b, cc, dd) = (
                    at(x0, y0),
                    at(x0 + 1, y0),
                    at(x0, y0 + 1),
                    at(x0 + 1, y0 + 1),
                );
                let ch = |sh: u32| {
                    let f = |p: u32| ((p >> sh) & 255) as f32;
                    let top = f(a) + (f(b) - f(a)) * tx;
                    let bot = f(cc) + (f(dd) - f(cc)) * tx;
                    (top + (bot - top) * ty).round() as u32
                };
                let (sa, sr, sg, sb) = (ch(24), ch(16), ch(8), ch(0));
                if sa == 0 {
                    continue;
                }
                let o = &mut self.buf[(y * self.width + x) as usize];
                let mix = |s: u32, dsh: u32| (s + ((*o >> dsh) & 255) * (255 - sa) / 255).min(255);
                *o = (mix(sr, 16) << 16) | (mix(sg, 8) << 8) | mix(sb, 0);
            }
        }
    }

    /// Draw one glyph with its pen at (`x`, `baseline`) in points.
    #[allow(clippy::too_many_arguments)]
    pub fn glyph(
        &mut self,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        face: FaceKey,
        id: quill_layout::GlyphId,
        size: f32,
        x: f32,
        baseline: f32,
        color: u32,
    ) {
        if let Some(g) = crate::color_glyph::get(fonts, face, id, size * self.scale) {
            self.picture(&g.pixels, [0.0, 0.0, 1.0, 1.0], g.rect(x, baseline, size));
            return;
        }
        let s = self.scale;
        let px = x * s;
        let ox = px.floor();
        let oy = (baseline * s).round() as i32;
        let m = cache.get(fonts, face, id, size * s, px - ox);
        self.mask(m, ox as i32, oy, color);
    }
}

pub struct ClipToken(Px);

/// Fill `r` with its corners rounded to radius `rad`, on any surface: the
/// straight middle as one block, the rounded ends in quarter-point strips.
pub fn round_rect(s: &mut dyn Surface, r: Rect, rad: f32, color: u32) {
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    s.fill(Rect::new(r.x, r.y + rad, r.w, r.h - 2.0 * rad), color);
    let step: f32 = 0.25;
    let mut d = 0.0;
    while d < rad {
        let h = step.min(rad - d);
        // How far in the curve is at the middle of this strip.
        let dy = rad - (d + h / 2.0);
        let inset = rad - (rad * rad - dy * dy).max(0.0).sqrt();
        let w = r.w - 2.0 * inset;
        s.fill(Rect::new(r.x + inset, r.y + d, w, h), color);
        s.fill(Rect::new(r.x + inset, r.bottom() - d - h, w, h), color);
        d += h;
    }
}

/// Something the page-drawing code can draw on: the window's canvas, or a
/// page of a PDF. Coordinates are in points.
pub trait Surface {
    fn fill(&mut self, r: Rect, color: u32);
    /// The thinnest line worth drawing, in points.
    fn hair(&self) -> f32;
    /// One device pixel, in points: the finest line the surface can show.
    fn pixel(&self) -> f32 {
        self.hair()
    }
    #[allow(clippy::too_many_arguments)]
    fn glyph(
        &mut self,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        face: FaceKey,
        id: quill_layout::GlyphId,
        size: f32,
        x: f32,
        baseline: f32,
        color: u32,
    );
    /// A glyph drawn only for its look, like the offset copies that make
    /// outline and shadow styles: in a PDF it is a shape, not text, so the
    /// character is not found or copied twice.
    #[allow(clippy::too_many_arguments)]
    fn glyph_art(
        &mut self,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        face: FaceKey,
        id: quill_layout::GlyphId,
        size: f32,
        x: f32,
        baseline: f32,
        color: u32,
    ) {
        self.glyph(fonts, cache, face, id, size, x, baseline, color);
    }
    /// Limit drawing to `r` (within any clip already set) until `pop_clip`.
    fn push_clip(&mut self, r: Rect);
    fn pop_clip(&mut self);
    /// Drawing for the screen: selections, page edges and marks show.
    fn screen(&self) -> bool {
        true
    }

    /// Draw a picture, scaled and cropped, into `r`.
    fn picture(&mut self, pic: &quill_core::Picture, r: Rect);

    fn frame(&mut self, r: Rect, color: u32) {
        let t = self.hair();
        self.fill(Rect::new(r.x, r.y, r.w, t), color);
        self.fill(Rect::new(r.x, r.bottom() - t, r.w, t), color);
        self.fill(Rect::new(r.x, r.y, t, r.h), color);
        self.fill(Rect::new(r.right() - t, r.y, t, r.h), color);
    }

    fn hline(&mut self, x0: f32, x1: f32, y: f32, color: u32) {
        let t = self.hair();
        self.fill(Rect::new(x0, y, x1 - x0, t), color);
    }

    fn vline(&mut self, x: f32, y0: f32, y1: f32, color: u32) {
        let t = self.hair();
        self.fill(Rect::new(x, y0, t, y1 - y0), color);
    }

    /// One point on, one off.
    fn dotted_hline(&mut self, x0: f32, x1: f32, y: f32, color: u32) {
        let t = self.hair();
        let mut x = x0.floor();
        while x < x1 {
            if (x as i32) % 2 == 0 {
                self.fill(Rect::new(x, y, 1.0, t), color);
            }
            x += 1.0;
        }
    }
}

impl Surface for Canvas<'_> {
    fn screen(&self) -> bool {
        !self.printing
    }

    fn fill(&mut self, r: Rect, color: u32) {
        Canvas::fill(self, r, color)
    }

    fn hair(&self) -> f32 {
        Canvas::hair(self)
    }

    fn pixel(&self) -> f32 {
        1.0 / self.scale
    }

    fn glyph(
        &mut self,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        face: FaceKey,
        id: quill_layout::GlyphId,
        size: f32,
        x: f32,
        baseline: f32,
        color: u32,
    ) {
        Canvas::glyph(self, fonts, cache, face, id, size, x, baseline, color)
    }

    fn push_clip(&mut self, r: Rect) {
        let old = self.set_clip(r);
        self.clips.push(old.0);
    }

    fn pop_clip(&mut self) {
        if let Some(c) = self.clips.pop() {
            self.clip = c;
        }
    }

    fn frame(&mut self, r: Rect, color: u32) {
        Canvas::frame(self, r, color)
    }

    fn hline(&mut self, x0: f32, x1: f32, y: f32, color: u32) {
        Canvas::hline(self, x0, x1, y, color)
    }

    fn vline(&mut self, x: f32, y0: f32, y1: f32, color: u32) {
        Canvas::vline(self, x, y0, y1, color)
    }

    fn dotted_hline(&mut self, x0: f32, x1: f32, y: f32, color: u32) {
        Canvas::dotted_hline(self, x0, x1, y, color)
    }

    fn picture(&mut self, pic: &quill_core::Picture, r: Rect) {
        let src = crop_fractions(pic);
        // Pixels needed for the whole image at this size on screen.
        let need_w = r.w * self.scale / (src[2] - src[0]).max(0.01);
        let need_h = r.h * self.scale / (src[3] - src[1]).max(0.01);
        match crate::image::for_screen(pic, need_w, need_h) {
            Some(d) => Canvas::picture(self, &d, src, r),
            None => {
                // Cannot decode: a gray box with a cross, so it shows.
                Canvas::frame(self, r, color::GRAY_DARK);
            }
        }
    }
}

/// The part of a picture that shows, as fractions of its width and height:
/// left, top, right, bottom.
pub fn crop_fractions(pic: &quill_core::Picture) -> [f32; 4] {
    let c = &pic.crop;
    let k = quill_core::picture::CROP_WHOLE as f32;
    [
        c.left as f32 / k,
        c.top as f32 / k,
        1.0 - c.right as f32 / k,
        1.0 - c.bottom as f32 / k,
    ]
}

/// Simple single-line text for interface labels.
pub struct Label<'f> {
    pub fonts: &'f mut Fonts,
    pub cache: &'f mut GlyphCache,
}

impl Label<'_> {
    /// Width of `text` in points.
    pub fn width(&mut self, font: &str, size: f32, bold: bool, text: &str) -> f32 {
        let face = self.fonts.resolve(font, bold, false);
        let mut w = 0.0;
        for c in text.chars() {
            let (f, g) = self.fonts.glyph(face, c);
            w += self.fonts.advance(f, g, size);
        }
        w
    }

    /// Draw `text` with its baseline at `y`. Returns the width.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        font: &str,
        size: f32,
        bold: bool,
        x: f32,
        y: f32,
        text: &str,
        color: u32,
    ) -> f32 {
        self.draw_face(canvas, font, size, bold, false, x, y, text, color)
    }

    /// Like `draw`, in italic.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_italic(
        &mut self,
        canvas: &mut Canvas,
        font: &str,
        size: f32,
        bold: bool,
        x: f32,
        y: f32,
        text: &str,
        color: u32,
    ) -> f32 {
        self.draw_face(canvas, font, size, bold, true, x, y, text, color)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_face(
        &mut self,
        canvas: &mut Canvas,
        font: &str,
        size: f32,
        bold: bool,
        italic: bool,
        x: f32,
        y: f32,
        text: &str,
        color: u32,
    ) -> f32 {
        let face = self.fonts.resolve(font, bold, italic);
        let mut pen = x;
        for c in text.chars() {
            let (f, g) = self.fonts.glyph(face, c);
            canvas.glyph(self.fonts, self.cache, f, g, size, pen, y, color);
            pen += self.fonts.advance(f, g, size);
        }
        pen - x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_mode_turns_grays_over_and_keeps_strong_colors() {
        let gray = |c: u32| c & 0xFF;
        assert!(gray(dark_ink(color::WHITE)) < 0x30);
        assert!(gray(dark_ink(color::BLACK)) > 0xD0);
        // Lighter stays lighter than darker, the other way round.
        assert!(gray(dark_ink(color::GRAY)) < gray(dark_ink(color::GRAY_DARK)));
        assert_eq!(dark_ink(0x0A84FF), 0x0A84FF);
        assert!(dark_ink(color::HIGHLIGHT) & 0xFF < 0x80);
        // Only while drawing in dark mode.
        let mut buf = vec![0u32; 4];
        let mut c = Canvas::new(&mut buf, 2, 2, 1.0);
        c.fill(Rect::new(0.0, 0.0, 1.0, 1.0), color::WHITE);
        c.dark = true;
        c.fill(Rect::new(1.0, 0.0, 1.0, 1.0), color::WHITE);
        assert_eq!(buf[0], color::WHITE);
        assert_eq!(buf[1], dark_ink(color::WHITE));
    }

    #[test]
    fn fills_outside_the_canvas_do_nothing() {
        let mut buf = vec![0u32; 20 * 10];
        let mut c = Canvas::new(&mut buf, 20, 10, 1.0);
        c.fill(Rect::new(25.0, 9.0, 10.0, 3.0), 0xFF);
        c.fill(Rect::new(-15.0, 0.0, 10.0, 3.0), 0xFF);
        c.fill(Rect::new(18.0, 9.0, 10.0, 3.0), 0xFF);
        assert_eq!(buf.iter().filter(|&&p| p == 0xFF).count(), 2);
    }

    #[test]
    fn panel_shadow_is_soft_and_extends_beyond_the_panel() {
        let mut buf = vec![0xFFFFFFu32; 40 * 40];
        let mut c = Canvas::new(&mut buf, 40, 40, 1.0);
        c.panel_shadow(Rect::new(10.0, 10.0, 20.0, 20.0), 5.0);
        assert_eq!(buf[20 * 40 + 20], 0xFFFFFF); // opaque panel area is skipped
        assert!(buf[30 * 40 + 20] < 0xFFFFFF); // no clear shelf below it
        assert!(buf[34 * 40 + 20] < 0xFFFFFF); // diffuse halo below it
        assert_eq!(buf[0], 0xFFFFFF); // but not across the whole canvas
    }
}
