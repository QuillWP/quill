//! Pictures in formats Quill does not keep (GIF, BMP, TIFF and WebP) are
//! turned into PNG as they come in, so everything that shows, saves or
//! exports pictures treats them like any PNG, and the documents Quill
//! writes stay readable by other programs. PNG and JPEG are kept as they
//! are.

use quill_core::Picture;
use std::io::Cursor;
use std::sync::Arc;

/// The most pixels a picture may have, as for the pictures Quill shows.
const MAX_PIXELS: u64 = 32_000_000;

/// A picture from a file's bytes, in any format Quill reads.
pub fn picture(bytes: impl Into<Arc<[u8]>>) -> Option<Picture> {
    let bytes = bytes.into();
    if let Some(p) = Picture::from_shared(bytes.clone()) {
        return Some(p);
    }
    let (w, h, rgba, dpi) = decode(&bytes)?;
    Picture::from_bytes(crate::png::encode_compact(w, h, &rgba, Some(dpi))?)
}

/// A device-independent bitmap without its file header, as RTF's
/// `\dibitmap` and the Windows clipboard carry it.
pub fn from_dib(dib: &[u8]) -> Option<Picture> {
    let header = le32(dib, 0)? as usize;
    let (bpp, compression, colors) = if header == 12 {
        (le16(dib, 10)?, 0, 0)
    } else {
        (le16(dib, 14)?, le32(dib, 16)?, le32(dib, 32)?)
    };
    let entry = if header == 12 { 3 } else { 4 };
    let palette = if bpp <= 8 {
        if colors == 0 || colors > 256 {
            1usize << bpp
        } else {
            colors as usize
        }
    } else {
        0
    };
    let masks = match (header, compression) {
        (40, 3) => 12,
        (40, 6) => 16,
        _ => 0,
    };
    let offset = 14 + header + masks + palette * entry;
    let mut file = Vec::with_capacity(14 + dib.len());
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&u32::try_from(14 + dib.len()).ok()?.to_le_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&u32::try_from(offset).ok()?.to_le_bytes());
    file.extend_from_slice(dib);
    picture(file)
}

/// Pixels (straight RGBA) and resolution of a GIF, BMP, TIFF or WebP.
fn decode(b: &[u8]) -> Option<(u32, u32, Vec<u8>, f32)> {
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        gif(b).map(|(w, h, px)| (w, h, px, 96.0))
    } else if b.starts_with(b"BM") {
        bmp(b)
    } else if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        tiff(b)
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        webp(b).map(|(w, h, px)| (w, h, px, 96.0))
    } else {
        None
    }
}

/// Whether a `w` by `h` picture is small enough to decode.
fn fits(w: u32, h: u32) -> bool {
    w > 0 && h > 0 && u64::from(w) * u64::from(h) <= MAX_PIXELS
}

/// A GIF's first frame, on a clear canvas the size of the GIF.
fn gif(b: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    opts.set_memory_limit(gif::MemoryLimit::Bytes(std::num::NonZeroU64::new(
        MAX_PIXELS * 4,
    )?));
    let mut d = opts.read_info(Cursor::new(b)).ok()?;
    let (w, h) = (u32::from(d.width()), u32::from(d.height()));
    if !fits(w, h) {
        return None;
    }
    let mut canvas = vec![0u8; (w * h * 4) as usize];
    let frame = d.read_next_frame().ok()??;
    let (fw, fh) = (usize::from(frame.width), usize::from(frame.height));
    let (left, top) = (usize::from(frame.left), usize::from(frame.top));
    for y in 0..fh {
        for x in 0..fw {
            let (cx, cy) = (left + x, top + y);
            if cx >= w as usize || cy >= h as usize {
                continue;
            }
            let src = (y * fw + x) * 4;
            let Some(px) = frame.buffer.get(src..src + 4) else {
                continue;
            };
            if px[3] != 0 {
                let dst = (cy * w as usize + cx) * 4;
                canvas[dst..dst + 4].copy_from_slice(px);
            }
        }
    }
    Some((w, h, canvas))
}

fn le16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// A Windows bitmap: 1, 4, 8, 16, 24 or 32 bits a pixel, uncompressed,
/// run-length encoded (RLE8, RLE4) or with bit field masks.
fn bmp(b: &[u8]) -> Option<(u32, u32, Vec<u8>, f32)> {
    let offset = le32(b, 10)? as usize;
    let header = le32(b, 14)? as usize;
    let (w, h, bpp, compression, ppm, colors) = if header == 12 {
        (
            i32::from(le16(b, 18)?),
            i32::from(le16(b, 20)?),
            le16(b, 24)?,
            0,
            0,
            0,
        )
    } else if header >= 40 {
        (
            le32(b, 18)? as i32,
            le32(b, 22)? as i32,
            le16(b, 28)?,
            le32(b, 30)?,
            le32(b, 38)?,
            le32(b, 46)?,
        )
    } else {
        return None;
    };
    let top_down = h < 0;
    let (w, h) = (w.unsigned_abs(), h.unsigned_abs());
    if w == 0 || !fits(w, h) || !matches!(bpp, 1 | 4 | 8 | 16 | 24 | 32) {
        return None;
    }
    // The palette follows the header: 3 bytes an entry in the oldest
    // bitmaps, 4 (blue, green, red, unused) in the rest.
    let entry = if header == 12 { 3 } else { 4 };
    let mut masks_end = 14 + header;
    // Bit field masks come after a 40-byte header, inside a longer one.
    let masks = if compression == 3 || compression == 6 {
        let at = if header == 40 { 14 + header } else { 54 };
        if header == 40 {
            masks_end += if compression == 6 { 16 } else { 12 };
        }
        let a = if compression == 6 || header >= 56 {
            le32(b, at + 12).unwrap_or(0)
        } else {
            0
        };
        Some([le32(b, at)?, le32(b, at + 4)?, le32(b, at + 8)?, a])
    } else {
        None
    };
    let palette: Vec<[u8; 4]> = if bpp <= 8 {
        let n = if colors == 0 || colors > 256 {
            1usize << bpp
        } else {
            colors as usize
        };
        (0..n)
            .map(|k| {
                let at = masks_end + k * entry;
                let e = b.get(at..at + 3).unwrap_or(&[0, 0, 0]);
                [e[2], e[1], e[0], 255]
            })
            .collect()
    } else {
        Vec::new()
    };
    let pixels = b.get(offset..)?;
    let (wu, hu) = (w as usize, h as usize);
    let mut out = vec![0u8; wu * hu * 4];
    // Rows run bottom to top unless the height is negative.
    let row_at = |y: usize| if top_down { y } else { hu - 1 - y };
    let put = |out: &mut Vec<u8>, x: usize, y: usize, c: [u8; 4]| {
        if x < wu && y < hu {
            let at = (row_at(y) * wu + x) * 4;
            out[at..at + 4].copy_from_slice(&c);
        }
    };
    let color = |k: usize| palette.get(k).copied().unwrap_or([0, 0, 0, 255]);
    match compression {
        1 | 2 => {
            // Run-length encoded, bottom-up, 8 or 4 bits a pixel.
            let four = compression == 2;
            let (mut x, mut y, mut i) = (0usize, 0usize, 0usize);
            while i + 1 < pixels.len() && y < hu {
                let (n, v) = (pixels[i] as usize, pixels[i + 1]);
                i += 2;
                if n > 0 {
                    for k in 0..n {
                        let idx = if four {
                            if k % 2 == 0 { v >> 4 } else { v & 15 }
                        } else {
                            v
                        };
                        put(&mut out, x, y, color(idx as usize));
                        x += 1;
                    }
                    continue;
                }
                match v {
                    0 => {
                        x = 0;
                        y += 1;
                    }
                    1 => break,
                    2 => {
                        let (dx, dy) = (*pixels.get(i)? as usize, *pixels.get(i + 1)? as usize);
                        i += 2;
                        x += dx;
                        y += dy;
                    }
                    n => {
                        let n = n as usize;
                        let bytes = if four { n.div_ceil(2) } else { n };
                        let run = pixels.get(i..i + bytes)?;
                        for k in 0..n {
                            let idx = if four {
                                let byte = run[k / 2];
                                if k % 2 == 0 { byte >> 4 } else { byte & 15 }
                            } else {
                                run[k]
                            };
                            put(&mut out, x, y, color(idx as usize));
                            x += 1;
                        }
                        // Absolute runs end on a 2-byte boundary.
                        i += bytes + bytes % 2;
                    }
                }
            }
        }
        0 | 3 | 6 => {
            let stride = (wu * bpp as usize).div_ceil(32) * 4;
            // Mask, shift and width of each channel for 16 and 32 bits.
            let fields = masks.unwrap_or(match bpp {
                16 => [0x7C00, 0x03E0, 0x001F, 0],
                _ => [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0],
            });
            let channel = |v: u32, mask: u32| -> u8 {
                if mask == 0 {
                    return 255;
                }
                let shift = mask.trailing_zeros();
                let bits = (mask >> shift).count_ones();
                let c = (v & mask) >> shift;
                let max = (1u64 << bits) - 1;
                ((u64::from(c) * 255 + max / 2) / max) as u8
            };
            let mut alpha_seen = false;
            for y in 0..hu {
                let row = pixels.get(y * stride..y * stride + stride)?;
                for x in 0..wu {
                    let c = match bpp {
                        1 | 4 | 8 => {
                            let bit = x * bpp as usize;
                            let byte = row[bit / 8];
                            let shift = 8 - bpp as usize - bit % 8;
                            color(((byte >> shift) & ((1u16 << bpp) - 1) as u8) as usize)
                        }
                        24 => [row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255],
                        16 | 32 => {
                            let v = if bpp == 16 {
                                u32::from(le16(row, x * 2)?)
                            } else {
                                le32(row, x * 4)?
                            };
                            let a = if fields[3] != 0 {
                                channel(v, fields[3])
                            } else {
                                255
                            };
                            alpha_seen |= fields[3] != 0 && a != 0;
                            [
                                channel(v, fields[0]),
                                channel(v, fields[1]),
                                channel(v, fields[2]),
                                a,
                            ]
                        }
                        _ => return None,
                    };
                    put(&mut out, x, y, c);
                }
            }
            // An alpha mask with every pixel clear means no alpha at all.
            if fields[3] != 0 && !alpha_seen {
                for px in out.as_chunks_mut::<4>().0 {
                    px[3] = 255;
                }
            }
        }
        _ => return None,
    }
    let dpi = if ppm > 0 { ppm as f32 * 0.0254 } else { 96.0 };
    Some((w, h, out, dpi))
}

/// A TIFF's first image: gray, palette, RGB, RGBA or CMYK, 1 to 16 bits a
/// channel, uncompressed or with LZW, Deflate, PackBits or fax coding.
fn tiff(b: &[u8]) -> Option<(u32, u32, Vec<u8>, f32)> {
    use tiff::decoder::Decoder;
    use tiff::tags::Tag;
    // The tiff crate reads no palette images. Their samples are color
    // indexes, so a copy marked gray reads them as levels, which the
    // color map then turns into colors.
    let mut d = Decoder::new(Cursor::new(b)).ok()?;
    if d.get_tag_u32(Tag::PhotometricInterpretation).ok()? == 3 {
        let map = d.get_tag_u16_vec(Tag::ColorMap).ok()?;
        let n = map.len() / 3;
        let lut: Vec<[u8; 4]> = (0..n)
            .map(|i| {
                [
                    (map[i] >> 8) as u8,
                    (map[n + i] >> 8) as u8,
                    (map[2 * n + i] >> 8) as u8,
                    255,
                ]
            })
            .collect();
        return tiff_pixels(&as_gray(b)?, Some(&lut));
    }
    tiff_pixels(b, None)
}

/// A copy of a TIFF with its first image marked gray (black is zero).
fn as_gray(b: &[u8]) -> Option<Vec<u8>> {
    let le = b.starts_with(b"II");
    let u16_at = |i: usize| {
        let v: [u8; 2] = b.get(i..i + 2)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(v)
        } else {
            u16::from_be_bytes(v)
        })
    };
    let u32_at = |i: usize| {
        let v: [u8; 4] = b.get(i..i + 4)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(v)
        } else {
            u32::from_be_bytes(v)
        })
    };
    // Classic TIFF only; BigTIFF palette pictures stay unread.
    if u16_at(2)? != 42 {
        return None;
    }
    let ifd = u32_at(4)? as usize;
    for k in 0..usize::from(u16_at(ifd)?) {
        let at = ifd + 2 + k * 12;
        if u16_at(at)? == 262 {
            let mut out = b.to_vec();
            let one = if le {
                1u16.to_le_bytes()
            } else {
                1u16.to_be_bytes()
            };
            out.get_mut(at + 8..at + 10)?.copy_from_slice(&one);
            return Some(out);
        }
    }
    None
}

/// A TIFF's pixels; `palette` turns gray levels into colors.
fn tiff_pixels(b: &[u8], palette: Option<&[[u8; 4]]>) -> Option<(u32, u32, Vec<u8>, f32)> {
    use tiff::ColorType;
    use tiff::decoder::{Decoder, DecodingResult, Limits};
    use tiff::tags::Tag;
    let mut limits = Limits::default();
    limits.decoding_buffer_size = (MAX_PIXELS * 8) as usize;
    limits.intermediate_buffer_size = (MAX_PIXELS * 8) as usize;
    let mut d = Decoder::new(Cursor::new(b)).ok()?.with_limits(limits);
    let (w, h) = d.dimensions().ok()?;
    if !fits(w, h) {
        return None;
    }
    let color = d.colortype().ok()?;
    // 0: white is zero, as fax pages are stored.
    let white_zero = d
        .get_tag_u32(Tag::PhotometricInterpretation)
        .is_ok_and(|p| p == 0);
    let dpi = match d.get_tag(Tag::XResolution) {
        Ok(tiff::decoder::ifd::Value::Rational(n, k)) if k > 0 => Some(f64::from(n) / f64::from(k)),
        Ok(v) => v.into_f64().ok(),
        Err(_) => None,
    }
    .filter(|r| r.is_finite() && *r > 0.0)
    .map(|r| match d.get_tag_u32(Tag::ResolutionUnit) {
        Ok(3) => r * 2.54,
        _ => r,
    })
    .unwrap_or(96.0) as f32;
    let image = d.read_image().ok()?;
    let (wu, hu) = (w as usize, h as usize);
    // Samples as 8 bits, whatever their stored width.
    let samples: Vec<u8> = match image {
        DecodingResult::U8(v) => v,
        DecodingResult::U16(v) => v.iter().map(|s| (s >> 8) as u8).collect(),
        _ => return None,
    };
    let mut out = Vec::with_capacity(wu * hu * 4);
    let gray = |v: u8| if white_zero { 255 - v } else { v };
    match color {
        ColorType::Gray(bits @ (1 | 2 | 4)) => {
            let stride = (wu * bits as usize).div_ceil(8);
            let levels = (1u16 << bits) - 1;
            for y in 0..hu {
                let row = samples.get(y * stride..y * stride + stride)?;
                for x in 0..wu {
                    let bit = x * bits as usize;
                    let v = (row[bit / 8] >> (8 - bits as usize - bit % 8)) & levels as u8;
                    match palette {
                        Some(lut) => out.extend_from_slice(
                            &lut.get(v as usize).copied().unwrap_or([0, 0, 0, 255]),
                        ),
                        None => {
                            let g = gray((u16::from(v) * 255 / levels) as u8);
                            out.extend_from_slice(&[g, g, g, 255]);
                        }
                    }
                }
            }
        }
        ColorType::Gray(_) => {
            for &g in samples.iter().take(wu * hu) {
                match palette {
                    Some(lut) => out
                        .extend_from_slice(&lut.get(g as usize).copied().unwrap_or([0, 0, 0, 255])),
                    None => {
                        let g = gray(g);
                        out.extend_from_slice(&[g, g, g, 255]);
                    }
                }
            }
        }
        ColorType::GrayA(_) => {
            for px in samples.as_chunks::<2>().0.iter().take(wu * hu) {
                let g = gray(px[0]);
                out.extend_from_slice(&[g, g, g, px[1]]);
            }
        }
        ColorType::RGB(_) => {
            for px in samples.as_chunks::<3>().0.iter().take(wu * hu) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        ColorType::RGBA(_) => out.extend(samples.iter().take(wu * hu * 4)),
        ColorType::CMYK(_) => {
            for px in samples.as_chunks::<4>().0.iter().take(wu * hu) {
                let k = 255 - u16::from(px[3]);
                let c = |v: u8| ((255 - u16::from(v)) * k / 255) as u8;
                out.extend_from_slice(&[c(px[0]), c(px[1]), c(px[2]), 255]);
            }
        }
        _ => return None,
    }
    (out.len() == wu * hu * 4).then_some((w, h, out, dpi))
}

/// A WebP picture, lossy or lossless; the first frame of an animation.
fn webp(b: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut d = image_webp::WebPDecoder::new(Cursor::new(b)).ok()?;
    let (w, h) = d.dimensions();
    if !fits(w, h) {
        return None;
    }
    let mut buf = vec![0u8; d.output_buffer_size()?];
    d.read_image(&mut buf).ok()?;
    if d.has_alpha() {
        Some((w, h, buf))
    } else {
        let mut out = Vec::with_capacity(buf.len() / 3 * 4);
        for px in buf.as_chunks::<3>().0.iter() {
            out.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
        Some((w, h, out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RGB: [[u8; 4]; 6] = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
        [0, 0, 0, 255],
        [255, 255, 255, 255],
    ];

    fn pixels(name: &str, bytes: &[u8]) -> (u32, u32, Vec<[u8; 4]>, f32) {
        let (w, h, px, dpi) = decode(bytes).unwrap_or_else(|| panic!("{name} did not decode"));
        let px = px.as_chunks::<4>().0.to_vec();
        (w, h, px, dpi)
    }

    #[test]
    fn every_format_decodes_to_the_same_pixels() {
        for (name, bytes) in [
            ("rgb.gif", &include_bytes!("../tests/pictures/rgb.gif")[..]),
            ("rgb24.bmp", include_bytes!("../tests/pictures/rgb24.bmp")),
            ("pal8.bmp", include_bytes!("../tests/pictures/pal8.bmp")),
            ("plain.tif", include_bytes!("../tests/pictures/plain.tif")),
            ("lzw.tif", include_bytes!("../tests/pictures/lzw.tif")),
            (
                "packbits.tif",
                include_bytes!("../tests/pictures/packbits.tif"),
            ),
            (
                "deflate.tif",
                include_bytes!("../tests/pictures/deflate.tif"),
            ),
            ("cmyk.tif", include_bytes!("../tests/pictures/cmyk.tif")),
            (
                "lossless.webp",
                include_bytes!("../tests/pictures/lossless.webp"),
            ),
        ] {
            let (w, h, px, _) = pixels(name, bytes);
            assert_eq!((w, h), (3, 2), "{name}");
            assert_eq!(px, RGB, "{name}");
        }
    }

    #[test]
    fn alpha_and_one_bit_pictures() {
        for (name, bytes) in [
            (
                "alpha.webp",
                &include_bytes!("../tests/pictures/alpha.webp")[..],
            ),
            ("alpha.tif", include_bytes!("../tests/pictures/alpha.tif")),
        ] {
            let (_, _, px, _) = pixels(name, bytes);
            assert_eq!(px[..5], RGB[..5], "{name}");
            assert_eq!(px[5][3], 0, "{name}");
        }
        // Black and white, the fax page stored white-is-zero.
        let (_, _, bmp, _) = pixels("mono.bmp", include_bytes!("../tests/pictures/mono.bmp"));
        let (_, _, fax, _) = pixels("fax.tif", include_bytes!("../tests/pictures/fax.tif"));
        assert_eq!(bmp, fax);
        assert_eq!((bmp[4], bmp[5]), ([0, 0, 0, 255], [255, 255, 255, 255]));
    }

    #[test]
    fn resolution_is_kept_and_the_result_is_a_png() {
        let (_, _, _, dpi) = pixels("plain.tif", include_bytes!("../tests/pictures/plain.tif"));
        assert_eq!(dpi.round(), 150.0);
        let pic = picture(include_bytes!("../tests/pictures/plain.tif").to_vec()).unwrap();
        assert_eq!(pic.format, quill_core::ImageFormat::Png);
        // 3 pixels at 150 dpi.
        assert_eq!(pic.natural.0, 29);
        // PNG and JPEG are kept byte for byte.
        let png = crate::png::encode(1, 1, &[0]);
        assert_eq!(&picture(png.clone()).unwrap().data[..], &png[..]);
    }

    #[test]
    fn run_length_bitmaps() {
        // 3 by 2, 8 bits, RLE8: a run of 3 of color 1, end of line, then
        // an absolute run of colors 0 1 0, end of bitmap.
        let mut b = b"BM".to_vec();
        let pixels: &[u8] = &[3, 1, 0, 0, 0, 3, 0, 1, 0, 0, 0, 1];
        let offset = 14 + 40 + 8;
        b.extend_from_slice(&((offset + pixels.len()) as u32).to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&(offset as u32).to_le_bytes());
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&3i32.to_le_bytes());
        b.extend_from_slice(&2i32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&8u16.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&[0; 12]);
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&[0, 0, 0, 0, 0, 0, 255, 0]);
        b.extend_from_slice(pixels);
        let (w, h, px, _) = pixels_of(&b);
        assert_eq!((w, h), (3, 2));
        let red = [255, 0, 0, 255];
        let black = [0, 0, 0, 255];
        // Bottom-up: the first line read is the bottom row.
        assert_eq!(px, [black, red, black, red, red, red]);
    }

    fn pixels_of(b: &[u8]) -> (u32, u32, Vec<[u8; 4]>, f32) {
        pixels("rle", b)
    }

    #[test]
    fn palette_tiffs_read_through_their_color_map() {
        let (w, h, px, _) = pixels(
            "palette.tif",
            include_bytes!("../tests/pictures/palette.tif"),
        );
        assert_eq!((w, h), (721, 374));
        // Not all one color, as it would be if the map were ignored.
        assert!(px.iter().any(|p| p[0] != p[1] || p[1] != p[2]));
    }

    #[test]
    fn a_bitmap_without_its_file_header() {
        let file = include_bytes!("../tests/pictures/pal8.bmp");
        let pic = from_dib(&file[14..]).unwrap();
        assert_eq!(pic.pixels, (3, 2));
    }

    #[test]
    fn hostile_headers_are_refused_without_large_allocations() {
        let mut b = include_bytes!("../tests/pictures/rgb24.bmp").to_vec();
        b[18..22].copy_from_slice(&100_000i32.to_le_bytes());
        b[22..26].copy_from_slice(&100_000i32.to_le_bytes());
        assert!(decode(&b).is_none());
        assert!(decode(b"GIF89a").is_none());
        assert!(decode(b"II*\0garbage").is_none());
        assert!(decode(b"RIFF\0\0\0\0WEBPVP8 ").is_none());
    }
}
