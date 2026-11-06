//! Bitmap color glyphs (including Apple Color Emoji), shared by screen and PDF.
//! Outline-only fonts continue through the normal vector/text path.

use crate::{
    canvas::Rect,
    image::{Decoded, decode_rgba},
};
use quill_core::{ImageFormat, Picture};
use quill_layout::{
    FaceKey, Fonts, GlyphId,
    ab_glyph::{Font, GlyphImageFormat},
};
use std::{cell::RefCell, collections::HashMap, sync::Arc};

pub struct ColorGlyph {
    pub pixels: Decoded,
    pub picture: Picture,
    pub left: f32,
    pub top: f32,
    pub em: f32,
}
impl ColorGlyph {
    pub fn rect(&self, x: f32, baseline: f32, size: f32) -> Rect {
        let scale = size / self.em;
        Rect::new(
            x + self.left * scale,
            baseline + self.top * scale,
            self.pixels.w as f32 * scale,
            self.pixels.h as f32 * scale,
        )
    }
}

type Key = (usize, u32, u16, u16);
#[derive(Default)]
struct Cache {
    items: HashMap<Key, Option<Arc<ColorGlyph>>>,
    bytes: usize,
}
thread_local! { static CACHE: RefCell<Cache> = RefCell::new(Cache::default()); }

pub fn get(fonts: &Fonts, face: FaceKey, glyph: GlyphId, pixels: f32) -> Option<Arc<ColorGlyph>> {
    if !pixels.is_finite() || pixels <= 0.0 {
        return None;
    }
    let face = fonts.face(face.face);
    let ppem = pixels.ceil().clamp(1.0, 256.0) as u16;
    // Font bytes live for the process, unlike a face number reused by another catalog.
    let key = (face.data.as_ptr() as usize, face.index, glyph.0, ppem);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(value) = cache.items.get(&key) {
            return value.clone();
        }
        let result = (|| {
            let image = face.font.glyph_raster_image2(glyph, ppem)?;
            if image.pixels_per_em == 0
                || u64::from(image.width) * u64::from(image.height) > 1_048_576
            {
                return None;
            }
            let (w, h, rgba, png) = match image.format {
                GlyphImageFormat::Png => {
                    let (_, w, h, _, _) = quill_core::picture::image_info(image.data)?;
                    if u64::from(w) * u64::from(h) > 1_048_576 {
                        return None;
                    }
                    let (w, h, rgba) = decode_rgba(image.data, ImageFormat::Png)?;
                    if u64::from(w) * u64::from(h) > 1_048_576 {
                        return None;
                    }
                    (w, h, rgba, image.data.to_vec())
                }
                GlyphImageFormat::BitmapPremulBgra32 => {
                    let (w, h) = (u32::from(image.width), u32::from(image.height));
                    if image.data.len() != w as usize * h as usize * 4 {
                        return None;
                    }
                    let mut rgba = Vec::with_capacity(image.data.len());
                    for c in image.data.as_chunks::<4>().0 {
                        let a = u32::from(c[3]);
                        let straight =
                            |v: u8| (u32::from(v) * 255).checked_div(a).unwrap_or(0).min(255) as u8;
                        rgba.extend_from_slice(&[
                            straight(c[2]),
                            straight(c[1]),
                            straight(c[0]),
                            c[3],
                        ]);
                    }
                    let png = quill_formats::png::encode_compact(w, h, &rgba, None)?;
                    (w, h, rgba, png)
                }
                _ => return None,
            };
            let px = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| {
                    let a = u32::from(c[3]);
                    let p = |i| (u32::from(c[i]) * a + 127) / 255;
                    a << 24 | p(0) << 16 | p(1) << 8 | p(2)
                })
                .collect();
            // sbix records use a bottom-left origin; CBDT/EBDT use a top bearing.
            let sbix = has_table(face.data, face.index, b"sbix");
            let top = if sbix {
                -image.origin.y - h as f32
            } else {
                -image.origin.y
            };
            Some(Arc::new(ColorGlyph {
                pixels: Decoded { w, h, px },
                picture: Picture::from_bytes(png)?,
                left: image.origin.x,
                top,
                em: f32::from(image.pixels_per_em),
            }))
        })();
        let bytes = result
            .as_ref()
            .map_or(0, |g| g.pixels.px.len() * 4 + g.picture.data.len());
        if cache.bytes + bytes > 8 << 20 || cache.items.len() >= 2048 {
            cache.items.clear();
            cache.bytes = 0;
        }
        cache.bytes += bytes;
        cache.items.insert(key, result.clone());
        result
    })
}

fn has_table(data: &[u8], index: u32, tag: &[u8; 4]) -> bool {
    let u32_at = |at: usize| {
        data.get(at..at + 4)
            .and_then(|s| s.try_into().ok())
            .map(u32::from_be_bytes)
    };
    let base = if data.starts_with(b"ttcf") {
        let Some(at) = u32_at(12 + index as usize * 4) else {
            return false;
        };
        at as usize
    } else {
        0
    };
    let Some(count) = data
        .get(base + 4..base + 6)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_be_bytes)
    else {
        return false;
    };
    (0..usize::from(count)).any(|i| data.get(base + 12 + i * 16..base + 16 + i * 16) == Some(tag))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_bitmap_emoji_has_nonempty_colored_pixels() {
        let mut fonts = Fonts::new(quill_layout::Catalog::scan_system());
        if !fonts.has_family("Apple Color Emoji") {
            return;
        }
        let face = fonts.resolve("Apple Color Emoji", false, false);
        let glyph = fonts.glyph_in(face, '\u{1f600}');
        let g = get(&fonts, face, glyph, 48.0).expect("decode installed color emoji");
        assert!(
            g.pixels
                .px
                .iter()
                .any(|&p| p >> 24 > 0 && (p >> 16 & 255) != (p >> 8 & 255))
        );
        let r = g.rect(10.0, 60.0, 48.0);
        assert!(r.y < 60.0 && r.bottom() > 10.0 && r.w > 0.0);
        assert!(Arc::ptr_eq(&g, &get(&fonts, face, glyph, 48.0).unwrap()));
    }
}
