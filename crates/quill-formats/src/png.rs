//! A minimal PNG writer: window snapshots, Quill's picture proxies, and
//! pictures in other formats turned into PNG as they come in.

use std::io::{self, Write};

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    // The checksum covers the chunk type and its data.
    let crc = crate::zip::crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encode 0x00RRGGBB pixels as an RGB PNG.
pub fn encode(width: u32, height: u32, pixels: &[u32]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width as usize * 3 + 1) * height as usize);
    for row in pixels.chunks(width as usize) {
        raw.push(0); // no filter
        for &p in row {
            raw.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Encode straight RGBA pixels with ordinary zlib compression. Native
/// documents use this for their small on-screen picture proxies.
pub fn encode_rgba(width: u32, height: u32, pixels: &[u8]) -> Option<Vec<u8>> {
    encode_rgba_at(width, height, pixels, None)
}

/// Like `encode_rgba`, recording `dpi` so the picture keeps its printed
/// size.
pub fn encode_rgba_at(width: u32, height: u32, pixels: &[u8], dpi: Option<f32>) -> Option<Vec<u8>> {
    let row = (width as usize).checked_mul(4)?;
    if pixels.len() != row.checked_mul(height as usize)? {
        return None;
    }
    let mut raw = Vec::with_capacity((row + 1).checked_mul(height as usize)?);
    for line in pixels.chunks(row) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    if let Some(dpi) = dpi.filter(|d| d.is_finite() && *d >= 1.0) {
        // Pixels per meter, the same both ways.
        let ppm = ((dpi / 0.0254).round() as u32).to_be_bytes();
        let mut phys = Vec::with_capacity(9);
        phys.extend_from_slice(&ppm);
        phys.extend_from_slice(&ppm);
        phys.push(1);
        chunk(&mut out, b"pHYs", &phys);
    }
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    Some(out)
}

/// Encode straight RGBA pixels as small as this writer can: a palette of
/// 1 to 8 bits a pixel when there are 256 colors or fewer, gray or RGB
/// without alpha when that is all the picture needs, and each other row
/// filtered the way that packs best. Pictures turned into PNG as they come
/// in use this, so they are no bigger than need be.
pub fn encode_compact(width: u32, height: u32, pixels: &[u8], dpi: Option<f32>) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || pixels.len() != w.checked_mul(h)?.checked_mul(4)? {
        return None;
    }
    let px = pixels.as_chunks::<4>().0;
    let opaque = px.iter().all(|p| p[3] == 255);
    let gray = px.iter().all(|p| p[0] == p[1] && p[1] == p[2]);
    // The colors, transparent ones first so tRNS stays short.
    let mut colors: Vec<[u8; 4]> = Vec::new();
    let mut index: std::collections::HashMap<[u8; 4], u8> = std::collections::HashMap::new();
    for p in px {
        if !index.contains_key(p) {
            if colors.len() == 256 {
                colors.clear();
                break;
            }
            index.insert(*p, 0);
            colors.push(*p);
        }
    }
    colors.sort_by_key(|c| c[3] == 255);
    for (k, c) in colors.iter().enumerate() {
        index.insert(*c, k as u8);
    }
    // Gray with many levels packs as well without a palette.
    let use_palette = !colors.is_empty() && !(gray && opaque && colors.len() > 16);
    let mut raw = Vec::new();
    let (depth, color_type);
    if use_palette {
        depth = match colors.len() {
            0..=2 => 1u8,
            3..=4 => 2,
            5..=16 => 4,
            _ => 8,
        };
        color_type = 3u8;
        let per_byte = 8 / depth as usize;
        let row_bytes = w.div_ceil(per_byte);
        raw.reserve((row_bytes + 1) * h);
        for row in px.chunks(w) {
            // Palette rows pack best unfiltered.
            raw.push(0);
            let mut line = vec![0u8; row_bytes];
            for (x, p) in row.iter().enumerate() {
                let i = index[p];
                let shift = 8 - depth as usize * (x % per_byte + 1);
                line[x / per_byte] |= i << shift;
            }
            raw.extend_from_slice(&line);
        }
    } else {
        depth = 8;
        color_type = match (gray, opaque) {
            (true, true) => 0,
            (true, false) => 4,
            (false, true) => 2,
            (false, false) => 6,
        };
        let channels: usize = match color_type {
            0 => 1,
            4 => 2,
            2 => 3,
            _ => 4,
        };
        let row_bytes = w * channels;
        let mut prev = vec![0u8; row_bytes];
        let mut line = Vec::with_capacity(row_bytes);
        let mut best = vec![0u8; row_bytes];
        let mut trial = vec![0u8; row_bytes];
        raw.reserve((row_bytes + 1) * h);
        for row in px.chunks(w) {
            line.clear();
            for p in row {
                match color_type {
                    0 => line.push(p[0]),
                    4 => line.extend_from_slice(&[p[0], p[3]]),
                    2 => line.extend_from_slice(&p[..3]),
                    _ => line.extend_from_slice(p),
                }
            }
            // The filter whose output has the smallest sum, as most PNG
            // writers choose.
            let mut best_filter = 0u8;
            let mut best_cost = u64::MAX;
            for filter in 0..5u8 {
                for i in 0..row_bytes {
                    let a = if i >= channels { line[i - channels] } else { 0 };
                    let b = prev[i];
                    let c = if i >= channels { prev[i - channels] } else { 0 };
                    let predicted = match filter {
                        0 => 0,
                        1 => a,
                        2 => b,
                        3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                        _ => paeth(a, b, c),
                    };
                    trial[i] = line[i].wrapping_sub(predicted);
                }
                let cost: u64 = trial
                    .iter()
                    .map(|&v| u64::from((v as i8).unsigned_abs()))
                    .sum();
                if cost < best_cost {
                    best_cost = cost;
                    best_filter = filter;
                    std::mem::swap(&mut best, &mut trial);
                }
            }
            raw.push(best_filter);
            raw.extend_from_slice(&best);
            prev.copy_from_slice(&line);
        }
    }
    // The strongest compression, except for very large pictures.
    let level = if raw.len() <= 16 << 20 { 9 } else { 6 };
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, level);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[depth, color_type, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    if use_palette {
        let plte: Vec<u8> = colors.iter().flat_map(|c| [c[0], c[1], c[2]]).collect();
        chunk(&mut out, b"PLTE", &plte);
        if !opaque {
            let trns: Vec<u8> = colors
                .iter()
                .take_while(|c| c[3] != 255)
                .map(|c| c[3])
                .collect();
            chunk(&mut out, b"tRNS", &trns);
        }
    }
    if let Some(dpi) = dpi.filter(|d| d.is_finite() && *d >= 1.0) {
        let ppm = ((dpi / 0.0254).round() as u32).to_be_bytes();
        let mut phys = Vec::with_capacity(9);
        phys.extend_from_slice(&ppm);
        phys.extend_from_slice(&ppm);
        phys.push(1);
        chunk(&mut out, b"pHYs", &phys);
    }
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    Some(out)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

pub fn write(path: &std::path::Path, width: u32, height: u32, pixels: &[u32]) -> io::Result<()> {
    std::fs::File::create(path)?.write_all(&encode(width, height, pixels))
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc_matches_reference() {
        // CRC of "IEND" with no data, as in every PNG.
        let crc = crate::zip::crc32(b"IEND");
        assert_eq!(crc, 0xAE42_6082);
    }

    #[test]
    fn encodes_header() {
        let png = super::encode(2, 1, &[0xFF0000, 0x00FF00]);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
    }
}
