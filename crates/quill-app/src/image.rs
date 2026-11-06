//! Turning pictures into pixels: our own PNG decoder (on the deflate code
//! the app already has) and zune-jpeg for JPEG. Pictures are kept on hand
//! at about the size the screen shows them, not at full resolution, so a
//! large photo does not hold tens of megabytes.

use quill_core::{Document, ImageFormat, Object, Picture};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

const MAX_DECODED_PIXELS: u64 = 32_000_000;
const PREVIEW_THRESHOLD_PIXELS: u64 = 4_000_000;
const PREVIEW_MAX_EDGE: u32 = 1600;

/// Pixels as premultiplied 0xAARRGGBB.
pub struct Decoded {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

/// Decode to straight RGBA, 8 bits a channel.
pub fn decode_rgba(data: &[u8], format: ImageFormat) -> Option<(u32, u32, Vec<u8>)> {
    match format {
        ImageFormat::Png => png(data),
        ImageFormat::Jpeg => jpeg(data),
    }
}

fn jpeg(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let opts = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_max_width(32_000)
        .set_max_height(32_000);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(
        zune_jpeg::zune_core::bytestream::ZCursor::new(data),
        opts,
    );
    d.decode_headers().ok()?;
    let info = d.info()?;
    let (w, h) = (info.width as u32, info.height as u32);
    if u64::from(w).checked_mul(u64::from(h))? > MAX_DECODED_PIXELS {
        return None;
    }
    let px = d.decode().ok()?;
    let need = usize::try_from(u64::from(w) * u64::from(h) * 4).ok()?;
    (px.len() == need).then_some((w, h, px))
}

fn be32(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(i..i + 4)?.try_into().ok()?))
}

fn png(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let (mut w, mut h, mut depth, mut ct, mut interlace) = (0, 0, 0u8, 0u8, 0u8);
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat = Vec::new();
    let mut i = 8;
    while i + 8 <= data.len() {
        let len = be32(data, i)? as usize;
        let kind = data.get(i + 4..i + 8)?;
        let body = data.get(i + 8..i + 8 + len)?;
        match kind {
            b"IHDR" => {
                if body.len() != 13 {
                    return None;
                }
                w = be32(body, 0)?;
                h = be32(body, 4)?;
                depth = *body.get(8)?;
                ct = *body.get(9)?;
                interlace = *body.get(12)?;
            }
            b"PLTE" => palette = body.as_chunks::<3>().0.to_vec(),
            b"tRNS" => trns = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        i += 12 + len;
    }
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_DECODED_PIXELS {
        return None;
    }
    let channels: u32 = match ct {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    let valid_depth = match ct {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        2 | 4 | 6 => matches!(depth, 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => false,
    };
    if !valid_depth || interlace > 1 {
        return None;
    }
    let bits = channels * depth as u32;
    let bpp = (bits as usize).div_ceil(8);
    let mut out = vec![0u8; usize::try_from(u64::from(w) * u64::from(h) * 4).ok()?];
    // Adam7 passes: start x, start y, step x, step y. One pass without
    // interlacing.
    let passes: &[(u32, u32, u32, u32)] = if interlace == 1 {
        &[
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ]
    } else {
        &[(0, 0, 1, 1)]
    };
    let raw_limit = passes.iter().try_fold(0usize, |sum, &(x0, y0, dx, dy)| {
        if x0 >= w || y0 >= h {
            return Some(sum);
        }
        let pw = (w - x0).div_ceil(dx) as usize;
        let ph = (h - y0).div_ceil(dy) as usize;
        let row = pw.checked_mul(bits as usize)?.div_ceil(8).checked_add(1)?;
        sum.checked_add(row.checked_mul(ph)?)
    })?;
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&idat, raw_limit).ok()?;
    if raw.len() != raw_limit {
        return None;
    }
    let mut at = 0;
    for &(x0, y0, dx, dy) in passes {
        if x0 >= w || y0 >= h {
            continue;
        }
        let pw = (w - x0).div_ceil(dx);
        let ph = (h - y0).div_ceil(dy);
        let stride = (pw as usize * bits as usize).div_ceil(8);
        let mut prev = vec![0u8; stride];
        for row in 0..ph {
            let filter = *raw.get(at)?;
            let mut line = raw.get(at + 1..at + 1 + stride)?.to_vec();
            at += 1 + stride;
            unfilter(filter, &mut line, &prev, bpp)?;
            let y = y0 + row * dy;
            for col in 0..pw {
                let x = x0 + col * dx;
                let rgba = pixel(&line, col as usize, ct, depth, &palette, &trns)?;
                let o = ((y * w + x) * 4) as usize;
                out[o..o + 4].copy_from_slice(&rgba);
            }
            prev = line;
        }
    }
    Some((w, h, out))
}

fn preview_rgba(w: u32, h: u32, rgba: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let scale = (w as f64 / PREVIEW_MAX_EDGE as f64)
        .max(h as f64 / PREVIEW_MAX_EDGE as f64)
        .max(1.0);
    let nw = (w as f64 / scale).round().max(1.0) as u32;
    let nh = (h as f64 / scale).round().max(1.0) as u32;
    let mut out = vec![0; usize::try_from(u64::from(nw) * u64::from(nh) * 4).ok()?];
    for y in 0..nh {
        let sy = ((u64::from(y) * u64::from(h)) / u64::from(nh)) as u32;
        for x in 0..nw {
            let sx = ((u64::from(x) * u64::from(w)) / u64::from(nw)) as u32;
            let from = usize::try_from((u64::from(sy) * u64::from(w) + u64::from(sx)) * 4).ok()?;
            let to = usize::try_from((u64::from(y) * u64::from(nw) + u64::from(x)) * 4).ok()?;
            out[to..to + 4].copy_from_slice(rgba.get(from..from + 4)?);
        }
    }
    Some((nw, nh, out))
}

/// Add a bounded screen proxy when decoding the original would be expensive.
/// Failure is harmless: the original remains available for export.
pub fn ensure_preview(pic: &mut Picture) {
    if pic.preview.is_some()
        || u64::from(pic.pixels.0) * u64::from(pic.pixels.1) <= PREVIEW_THRESHOLD_PIXELS
    {
        return;
    }
    let Some((w, h, rgba)) = decode_rgba(&pic.data, pic.format) else {
        return;
    };
    let Some((w, h, rgba)) = preview_rgba(w, h, &rgba) else {
        return;
    };
    let Some(bytes) = crate::png::encode_rgba(w, h, &rgba) else {
        return;
    };
    pic.preview = Some(bytes.into());
    pic.preview_pixels = Some((w, h));
}

/// Share originals and proxies between every placement in a document.
pub fn prepare_document_previews(doc: &mut Document) {
    type Seen = (Arc<[u8]>, Option<Arc<[u8]>>, Option<(u32, u32)>);
    let mut seen: Vec<Seen> = Vec::new();
    for object in &mut doc.objects {
        let Object::Picture(pic) = object else {
            continue;
        };
        if let Some((data, preview, pixels)) = seen.iter().find(|(d, _, _)| **d == *pic.data) {
            pic.data = data.clone();
            pic.preview = preview.clone();
            pic.preview_pixels = *pixels;
        } else {
            ensure_preview(pic);
            seen.push((pic.data.clone(), pic.preview.clone(), pic.preview_pixels));
        }
    }
}

fn unfilter(filter: u8, line: &mut [u8], prev: &[u8], bpp: usize) -> Option<()> {
    for i in 0..line.len() {
        let a = if i >= bpp { line[i - bpp] as i32 } else { 0 };
        let b = prev[i] as i32;
        let c = if i >= bpp { prev[i - bpp] as i32 } else { 0 };
        let add = match filter {
            0 => 0,
            1 => a,
            2 => b,
            3 => (a + b) / 2,
            4 => {
                let p = a + b - c;
                let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                }
            }
            _ => return None,
        };
        line[i] = line[i].wrapping_add(add as u8);
    }
    Some(())
}

/// One pixel of an unfiltered scanline as RGBA.
fn pixel(
    line: &[u8],
    x: usize,
    ct: u8,
    depth: u8,
    pal: &[[u8; 3]],
    trns: &[u8],
) -> Option<[u8; 4]> {
    let d = depth as usize;
    // Sample `k` of the pixel, at its own bit depth.
    let sample = |k: usize, n: usize| -> Option<u16> {
        if d == 16 {
            let i = (x * n + k) * 2;
            return Some(u16::from_be_bytes([*line.get(i)?, *line.get(i + 1)?]));
        }
        if d == 8 {
            return line.get(x * n + k).map(|&v| v as u16);
        }
        let bit = (x * n + k) * d;
        let byte = *line.get(bit / 8)?;
        let shift = 8 - d - bit % 8;
        Some(((byte >> shift) & ((1 << d) - 1) as u8) as u16)
    };
    let max = (1u32 << d) - 1;
    let to8 = |v: u16| -> u8 {
        if d == 16 {
            (v >> 8) as u8
        } else {
            (v as u32 * 255 / max) as u8
        }
    };
    let trns16 = |k: usize| -> Option<u16> {
        Some(u16::from_be_bytes([
            *trns.get(k * 2)?,
            *trns.get(k * 2 + 1)?,
        ]))
    };
    Some(match ct {
        0 => {
            let g = sample(0, 1)?;
            let a = if trns16(0) == Some(g) { 0 } else { 255 };
            let g = to8(g);
            [g, g, g, a]
        }
        2 => {
            let (r, g, b) = (sample(0, 3)?, sample(1, 3)?, sample(2, 3)?);
            let clear =
                trns.len() >= 6 && (trns16(0), trns16(1), trns16(2)) == (Some(r), Some(g), Some(b));
            [to8(r), to8(g), to8(b), if clear { 0 } else { 255 }]
        }
        3 => {
            let i = sample(0, 1)? as usize;
            let c = pal.get(i).copied().unwrap_or([0, 0, 0]);
            [c[0], c[1], c[2], trns.get(i).copied().unwrap_or(255)]
        }
        4 => {
            let g = to8(sample(0, 2)?);
            [g, g, g, to8(sample(1, 2)?)]
        }
        _ => [
            to8(sample(0, 4)?),
            to8(sample(1, 4)?),
            to8(sample(2, 4)?),
            to8(sample(3, 4)?),
        ],
    })
}

/// Shrink by a whole factor `k`, averaging each k x k block, into
/// premultiplied pixels.
fn shrink(w: u32, h: u32, rgba: &[u8], k: u32) -> Decoded {
    let (nw, nh) = (w.div_ceil(k), h.div_ceil(k));
    let mut px = Vec::with_capacity((nw * nh) as usize);
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0u32; 4];
            let mut n = 0;
            for yy in y * k..((y + 1) * k).min(h) {
                for xx in x * k..((x + 1) * k).min(w) {
                    let i = ((yy * w + xx) * 4) as usize;
                    let a = rgba[i + 3] as u32;
                    acc[0] += rgba[i] as u32 * a / 255;
                    acc[1] += rgba[i + 1] as u32 * a / 255;
                    acc[2] += rgba[i + 2] as u32 * a / 255;
                    acc[3] += a;
                    n += 1;
                }
            }
            let n = n.max(1);
            px.push(
                ((acc[3] / n) << 24) | ((acc[0] / n) << 16) | ((acc[1] / n) << 8) | (acc[2] / n),
            );
        }
    }
    Decoded { w: nw, h: nh, px }
}

type CacheEntry = (Arc<[u8]>, u32, Rc<Decoded>);

thread_local! {
    /// Recently drawn pictures: the encoded data itself, the shrink factor,
    /// and the pixels. Retaining the Arc makes identity stable: an allocator
    /// cannot reuse a freed picture's address for unrelated bytes.
    static CACHE: RefCell<Vec<CacheEntry>> = const { RefCell::new(Vec::new()) };
}

/// The picture's pixels, shrunk to no less than `need_w` x `need_h`
/// device pixels for its whole (uncropped) image.
pub fn for_screen(pic: &Picture, need_w: f32, need_h: f32) -> Option<Rc<Decoded>> {
    let (data, format, (w, h)) = match (&pic.preview, pic.preview_pixels) {
        (Some(data), Some(dimensions)) => (data.clone(), ImageFormat::Png, dimensions),
        _ => (pic.data.clone(), pic.format, pic.pixels),
    };
    let k = ((w as f32 / need_w.max(1.0)).min(h as f32 / need_h.max(1.0)))
        .floor()
        .clamp(1.0, 64.0) as u32;
    // Powers of two, so small changes in size do not decode again.
    let k = 1u32 << (31 - k.leading_zeros());
    if let Some(d) = CACHE.with(|c| {
        c.borrow()
            .iter()
            .find(|(cached, kk, _)| *kk == k && (Arc::ptr_eq(cached, &data) || **cached == *data))
            .map(|e| e.2.clone())
    }) {
        return Some(d);
    }
    let (w, h, rgba) = decode_rgba(&data, format)?;
    let d = Rc::new(shrink(w, h, &rgba, k));
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|(cached, _, _)| !Arc::ptr_eq(cached, &data) && **cached != *data);
        c.push((data, k, d.clone()));
        if c.len() > 12 {
            c.remove(0);
        }
    });
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_pngs_of_every_kind_decode_to_the_same_pixels() {
        let gray: Vec<u8> = (0..64u8).flat_map(|g| [g * 4, g * 4, g * 4, 255]).collect();
        let two: Vec<u8> = (0..64u8)
            .flat_map(|g| {
                if g % 3 == 0 {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                }
            })
            .collect();
        let few: Vec<u8> = (0..64u8)
            .flat_map(|g| [[255, 0, 0, 255], [0, 255, 0, 128], [0, 0, 255, 0]][g as usize % 3])
            .collect();
        let rgb: Vec<u8> = (0..64u32)
            .flat_map(|i| [(i * 4) as u8, (255 - i * 4) as u8, (i * 7) as u8, 255])
            .collect();
        let rgba: Vec<u8> = (0..64u32)
            .flat_map(|i| {
                [
                    (i * 4) as u8,
                    (255 - i * 4) as u8,
                    (i * 7) as u8,
                    (i * 3) as u8,
                ]
            })
            .collect();
        let gray_alpha: Vec<u8> = (0..64u8).flat_map(|g| [g * 4, g * 4, g * 4, g]).collect();
        // 8 by 8, and 7 by 9 so rows end part way through a byte.
        for (w, h) in [(8, 8), (7, 9)] {
            for (name, px) in [
                ("gray", &gray),
                ("two colors", &two),
                ("palette with alpha", &few),
                ("rgb", &rgb),
                ("rgba", &rgba),
                ("gray and alpha", &gray_alpha),
            ] {
                let px = &px[..(w * h * 4) as usize];
                let png = quill_formats::png::encode_compact(w, h, px, Some(150.0)).unwrap();
                let (dw, dh, back) = decode_rgba(&png, ImageFormat::Png).unwrap();
                assert_eq!((dw, dh), (w, h), "{name}");
                assert_eq!(back, px, "{name} {w}x{h}");
            }
        }
    }

    #[test]
    fn decodes_our_own_png() {
        // The snapshot encoder writes 8-bit RGB with stored deflate blocks.
        let px = [0xFF0000, 0x00FF00, 0x0000FF, 0xFFFFFF];
        let bytes = crate::png::encode(2, 2, &px);
        let (w, h, rgba) = png(&bytes).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[0, 255, 0, 255]);
        assert_eq!(&rgba[12..16], &[255, 255, 255, 255]);
    }

    #[test]
    fn unfilters_paeth_and_reads_low_bit_depths() {
        let mut line = vec![10, 20, 5, 5];
        unfilter(4, &mut line, &[1, 2, 3, 4], 2).unwrap();
        assert_eq!(line, [11, 22, 16, 27]);
        // 1-bit gray: 0b1010_0000 is white, black, white, black.
        let p = pixel(&[0b1010_0000], 2, 0, 1, &[], &[]).unwrap();
        assert_eq!(p, [255, 255, 255, 255]);
        let p = pixel(&[0b1010_0000], 1, 0, 1, &[], &[]).unwrap();
        assert_eq!(p, [0, 0, 0, 255]);
    }

    #[test]
    fn shrinks_by_averaging() {
        let rgba = [255, 255, 255, 255, 0, 0, 0, 255];
        let d = shrink(2, 1, &rgba, 2);
        assert_eq!(d.w, 1);
        assert_eq!(d.px[0], 0xFF7F7F7F);
    }

    #[test]
    fn rejects_invalid_or_oversized_png_headers_without_allocating() {
        let mut invalid = crate::png::encode(1, 1, &[0]);
        invalid[24] = 3; // RGB cannot have a 3-bit sample depth.
        assert!(png(&invalid).is_none());

        let mut huge = crate::png::encode(1, 1, &[0]);
        huge[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        assert!(png(&huge).is_none());
    }

    #[test]
    fn large_picture_uses_a_bounded_preview_for_screen() {
        let (w, h) = (2001, 2000);
        let pixels = vec![0x336699; (w * h) as usize];
        let mut pic = Picture::from_bytes(crate::png::encode(w, h, &pixels)).unwrap();
        ensure_preview(&mut pic);
        let (pw, ph) = pic.preview_pixels.unwrap();
        assert!(pw <= PREVIEW_MAX_EDGE && ph <= PREVIEW_MAX_EDGE);
        assert!(pic.preview.is_some());
        // Rendering no longer needs to touch the original after the proxy exists.
        pic.data = Arc::from(&b"not an image"[..]);
        assert!(for_screen(&pic, 200.0, 200.0).is_some());
    }

    #[test]
    fn cache_matches_encoded_content_not_an_allocator_address() {
        CACHE.with(|c| c.borrow_mut().clear());
        let bytes = crate::png::encode(2, 1, &[0x112233, 0x445566]);
        let first = Picture::from_bytes(bytes.clone()).unwrap();
        let second = Picture::from_bytes(bytes).unwrap();
        assert!(!Arc::ptr_eq(&first.data, &second.data));
        let a = for_screen(&first, 2.0, 1.0).unwrap();
        let b = for_screen(&second, 2.0, 1.0).unwrap();
        assert!(Rc::ptr_eq(&a, &b));
        CACHE.with(|c| {
            let cache = c.borrow();
            assert_eq!(cache.len(), 1);
            assert!(Arc::ptr_eq(&cache[0].0, &first.data));
        });
    }
}
