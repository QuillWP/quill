//! Turning glyph outlines into coverage masks, with a bounded cache.

use crate::fonts::{FaceKey, Fonts};
use ab_glyph::{Font, GlyphId, OutlineCurve, Point};
use ab_glyph_rasterizer::{Rasterizer, point};
use std::collections::HashMap;

/// Anti-aliased coverage of one glyph, 0-255 per pixel. `left` and `top`
/// place the mask relative to the pen position on the baseline (y grows
/// down, so `top` is usually negative).
#[derive(Clone, Debug, Default)]
pub struct Mask {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    face: FaceKey,
    glyph: u16,
    /// Pixel size in 1/16 px.
    px: u32,
    /// Horizontal sub-pixel position in quarters of a pixel.
    sub: u8,
}

/// How far synthetic italic leans: x moves this much per unit of height.
/// How far fake italics lean: x moves this much per unit of height.
pub const ITALIC_SHEAR: f32 = 0.2;

/// Cached masks are dropped when they pass this many bytes, which keeps
/// memory flat however many sizes and fonts a document uses.
const CACHE_LIMIT: usize = 4 << 20;

#[derive(Default)]
pub struct GlyphCache {
    map: HashMap<Key, Mask>,
    bytes: usize,
}

impl GlyphCache {
    pub fn new() -> Self {
        GlyphCache::default()
    }

    /// The mask for `glyph` at `px` pixels per em, drawn with its pen at
    /// `x_frac` (0..1) of a pixel.
    pub fn get(
        &mut self,
        fonts: &Fonts,
        face: FaceKey,
        glyph: GlyphId,
        px: f32,
        x_frac: f32,
    ) -> &Mask {
        let key = Key {
            face,
            glyph: glyph.0,
            px: (px * 16.0).round() as u32,
            sub: ((x_frac * 4.0).floor() as u8).min(3),
        };
        if !self.map.contains_key(&key) {
            if self.bytes > CACHE_LIMIT {
                self.map.clear();
                self.bytes = 0;
            }
            let mask = rasterize(fonts, face, glyph, px, key.sub as f32 / 4.0);
            self.bytes += mask.data.len() + 32;
            self.map.insert(key, mask);
        }
        &self.map[&key]
    }
}

fn rasterize(fonts: &Fonts, key: FaceKey, glyph: GlyphId, px: f32, sub: f32) -> Mask {
    let face = fonts.face(key.face);
    let Some(outline) = face.font.outline(glyph) else {
        return Mask::default();
    };
    let s = px / face.units_per_em;
    let shear = if key.fake_italic { ITALIC_SHEAR } else { 0.0 };
    // Font units have y up; masks have y down.
    let tf = |p: Point| point(p.x * s + p.y * s * shear + sub, -p.y * s);
    let b = outline.bounds;
    let corners = [
        tf(Point {
            x: b.min.x,
            y: b.min.y,
        }),
        tf(Point {
            x: b.max.x,
            y: b.min.y,
        }),
        tf(Point {
            x: b.min.x,
            y: b.max.y,
        }),
        tf(Point {
            x: b.max.x,
            y: b.max.y,
        }),
    ];
    let min_x = corners.iter().map(|p| p.x).fold(f32::MAX, f32::min).floor();
    let max_x = corners.iter().map(|p| p.x).fold(f32::MIN, f32::max).ceil();
    let min_y = corners.iter().map(|p| p.y).fold(f32::MAX, f32::min).floor();
    let max_y = corners.iter().map(|p| p.y).fold(f32::MIN, f32::max).ceil();
    let w = (max_x - min_x).max(0.0) as usize;
    let h = (max_y - min_y).max(0.0) as usize;
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return Mask::default();
    }
    let mut r = Rasterizer::new(w, h);
    let at = |p: Point| {
        let q = tf(p);
        point(q.x - min_x, q.y - min_y)
    };
    for curve in &outline.curves {
        match *curve {
            OutlineCurve::Line(a, b) => r.draw_line(at(a), at(b)),
            OutlineCurve::Quad(a, b, c) => r.draw_quad(at(a), at(b), at(c)),
            OutlineCurve::Cubic(a, b, c, d) => r.draw_cubic(at(a), at(b), at(c), at(d)),
        }
    }
    let mut data = vec![0u8; w * h];
    r.for_each_pixel(|i, v| data[i] = (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let mut mask = Mask {
        left: min_x as i32,
        top: min_y as i32,
        width: w as u32,
        height: h as u32,
        data,
    };
    if key.fake_bold {
        let d = (px / 24.0).round().max(1.0) as usize;
        mask = smear(&mask, d);
    }
    mask
}

/// Synthetic bold: overlay the glyph on itself shifted `d` pixels right.
fn smear(m: &Mask, d: usize) -> Mask {
    let (w, h) = (m.width as usize, m.height as usize);
    let nw = w + d;
    let mut data = vec![0u8; nw * h];
    for y in 0..h {
        let src = &m.data[y * w..y * w + w];
        let dst = &mut data[y * nw..y * nw + nw];
        for shift in 0..=d {
            for (x, &v) in src.iter().enumerate() {
                let o = &mut dst[x + shift];
                *o = (*o).max(v);
            }
        }
    }
    Mask {
        width: nw as u32,
        data,
        ..*m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::Catalog;

    #[test]
    fn rasterizes_a_letter() {
        let mut fonts = Fonts::new(Catalog::scan_system());
        if fonts.family_names().is_empty() {
            return;
        }
        let k = fonts.resolve("Helvetica", false, false);
        let (k, g) = fonts.glyph(k, 'H');
        let mut cache = GlyphCache::new();
        let m = cache.get(&fonts, k, g, 24.0, 0.0).clone();
        assert!(m.width > 8 && m.height > 12, "{}x{}", m.width, m.height);
        assert!(m.top < 0, "glyph sits above the baseline");
        assert!(m.data.contains(&255), "H has solid pixels");
        let bold = cache
            .get(
                &fonts,
                FaceKey {
                    fake_bold: true,
                    ..k
                },
                g,
                24.0,
                0.0,
            )
            .clone();
        assert!(bold.width > m.width);
    }
}
