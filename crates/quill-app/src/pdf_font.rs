//! Fonts for PDF text: a small TrueType font for each face a document
//! uses, holding only the glyphs it uses, built from the same outlines Quill
//! draws with. Building rather than copying the font file means a
//! variable font prints at the weight and optical size Quill uses, a face
//! inside a collection comes out on its own, and fonts with cubic
//! outlines work too (their curves turned into quadratic ones).
//!
//! The PDF refers to glyphs by their place in this font (1, 2, 3...; 0 is
//! the empty .notdef), with a ToUnicode map back to the characters so the
//! text can be searched and copied.

use std::collections::HashMap;
use std::fmt::Write as _;

use quill_layout::ab_glyph::{self, Font, GlyphId, OutlineCurve};
use quill_layout::{FaceId, Fonts};

/// Whether the font's license lets it be embedded: the OS/2 table's
/// fsType, read straight from the file. Restricted (bit 1) and
/// bitmap-only (bit 9) fonts are drawn as outlines instead.
pub fn embeddable(data: &[u8], index: u32) -> bool {
    let be16 = |i: usize| data.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let be32 = |i: usize| {
        data.get(i..i + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    // A collection points at each face's table directory.
    let dir = if data.starts_with(b"ttcf") {
        match be32(12 + 4 * index as usize) {
            Some(o) => o as usize,
            None => return false,
        }
    } else {
        0
    };
    let Some(count) = be16(dir + 4) else {
        return false;
    };
    for t in 0..count as usize {
        let rec = dir + 12 + 16 * t;
        if data.get(rec..rec + 4) == Some(b"OS/2") {
            let Some(off) = be32(rec + 8) else {
                return false;
            };
            let fs_type = be16(off as usize + 8).unwrap_or(0);
            return fs_type & 0x0002 == 0 && fs_type & 0x0200 == 0;
        }
    }
    // No OS/2 table: nothing forbids it.
    true
}

/// A face's glyphs in the PDF, in the order first used.
#[derive(Default)]
pub struct Subset {
    pub glyphs: Vec<GlyphId>,
    index: HashMap<GlyphId, u16>,
}

impl Subset {
    /// The glyph's number in the subset font, adding it on first use.
    pub fn cid(&mut self, id: GlyphId) -> u16 {
        if let Some(&c) = self.index.get(&id) {
            return c;
        }
        self.glyphs.push(id);
        let c = self.glyphs.len() as u16;
        self.index.insert(id, c);
        c
    }
}

/// What the PDF needs to describe a subset font, in its 1000-unit em.
pub struct Built {
    pub ttf: Vec<u8>,
    /// The width of each subset glyph, from glyph 1.
    pub widths: Vec<f32>,
    pub bbox: [f32; 4],
    pub ascent: f32,
    pub descent: f32,
}

/// One glyph as TrueType contours: points with on-curve flags.
struct Contours {
    points: Vec<(i16, i16, bool)>,
    ends: Vec<u16>,
}

fn round16(v: f32) -> i16 {
    v.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

/// A glyph's outline as TrueType contours. Cubic curves become quadratic
/// ones, four to each cubic, which is closer than a printer can show.
fn contours(curves: &[OutlineCurve]) -> Contours {
    let mut out = Contours {
        points: Vec::new(),
        ends: Vec::new(),
    };
    let mut start = 0usize;
    let mut last: Option<(i16, i16)> = None;
    let p = |q: ab_glyph::Point| (round16(q.x), round16(q.y));
    let close = |out: &mut Contours, start: usize| {
        // TrueType contours close themselves: drop a final point that
        // repeats the first.
        let n = out.points.len();
        if n > start + 1 && out.points[n - 1] == out.points[start] {
            out.points.pop();
        }
        if out.points.len() > start {
            out.ends.push((out.points.len() - 1) as u16);
        }
    };
    let mut segments: Vec<(ab_glyph::Point, Option<ab_glyph::Point>, ab_glyph::Point)> = Vec::new();
    for c in curves {
        match *c {
            OutlineCurve::Line(a, b) => segments.push((a, None, b)),
            OutlineCurve::Quad(a, q, b) => segments.push((a, Some(q), b)),
            OutlineCurve::Cubic(a, c1, c2, b) => {
                let at = |t: f32| {
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    ab_glyph::point(
                        w[0] * a.x + w[1] * c1.x + w[2] * c2.x + w[3] * b.x,
                        w[0] * a.y + w[1] * c1.y + w[2] * c2.y + w[3] * b.y,
                    )
                };
                // Each quarter of the cubic as a quadratic through the
                // same ends, its control where the tangents meet nearly.
                let d = |t: f32| {
                    let u = 1.0 - t;
                    ab_glyph::point(
                        3.0 * (u * u * (c1.x - a.x)
                            + 2.0 * u * t * (c2.x - c1.x)
                            + t * t * (b.x - c2.x)),
                        3.0 * (u * u * (c1.y - a.y)
                            + 2.0 * u * t * (c2.y - c1.y)
                            + t * t * (b.y - c2.y)),
                    )
                };
                for k in 0..4 {
                    let (t0, t1) = (k as f32 / 4.0, (k + 1) as f32 / 4.0);
                    let (p0, p1) = (at(t0), at(t1));
                    let (d0, d1) = (d(t0), d(t1));
                    let h = (t1 - t0) / 2.0;
                    // Average of the two tangent estimates of the control.
                    let q = ab_glyph::point(
                        ((p0.x + d0.x * h) + (p1.x - d1.x * h)) / 2.0,
                        ((p0.y + d0.y * h) + (p1.y - d1.y * h)) / 2.0,
                    );
                    segments.push((p0, Some(q), p1));
                }
            }
        }
    }
    for (a, q, b) in segments {
        let a = p(a);
        if last != Some(a) {
            if last.is_some() {
                close(&mut out, start);
            }
            start = out.points.len();
            out.points.push((a.0, a.1, true));
        }
        if let Some(q) = q {
            let q = p(q);
            out.points.push((q.0, q.1, false));
        }
        let b = p(b);
        out.points.push((b.0, b.1, true));
        last = Some(b);
    }
    if last.is_some() {
        close(&mut out, start);
    }
    out
}

/// A simple glyph record, and its bounding box.
fn glyph_record(c: &Contours) -> (Vec<u8>, [i16; 4]) {
    if c.ends.is_empty() {
        return (Vec::new(), [0; 4]);
    }
    let xs = c.points.iter().map(|p| p.0);
    let ys = c.points.iter().map(|p| p.1);
    let bbox = [
        xs.clone().min().unwrap_or(0),
        ys.clone().min().unwrap_or(0),
        xs.max().unwrap_or(0),
        ys.max().unwrap_or(0),
    ];
    let mut g = Vec::new();
    g.extend_from_slice(&(c.ends.len() as i16).to_be_bytes());
    for v in bbox {
        g.extend_from_slice(&v.to_be_bytes());
    }
    for e in &c.ends {
        g.extend_from_slice(&e.to_be_bytes());
    }
    g.extend_from_slice(&0u16.to_be_bytes()); // no instructions
    for p in &c.points {
        g.push(if p.2 { 1 } else { 0 });
    }
    // Coordinates as two-byte steps from the point before.
    let mut prev = 0i32;
    for p in &c.points {
        g.extend_from_slice(&((p.0 as i32 - prev) as i16).to_be_bytes());
        prev = p.0 as i32;
    }
    prev = 0;
    for p in &c.points {
        g.extend_from_slice(&((p.1 as i32 - prev) as i16).to_be_bytes());
        prev = p.1 as i32;
    }
    while g.len() % 4 != 0 {
        g.push(0);
    }
    (g, bbox)
}

fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut b = [0u8; 4];
        b[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(b));
    }
    sum
}

/// Build the subset font for `face` holding `glyphs` (glyph 0, .notdef,
/// is added in front and left empty).
pub fn build(fonts: &Fonts, face: FaceId, glyphs: &[GlyphId]) -> Built {
    let lf = fonts.face(face);
    let font = &lf.font;
    let upem = lf.units_per_em.round().clamp(16.0, 16384.0);
    let scale = 1000.0 / upem;
    let mut records = vec![Vec::new()];
    let mut boxes = vec![[0i16; 4]];
    let mut advances = vec![0u16];
    let (mut max_points, mut max_contours) = (0u16, 0u16);
    for &id in glyphs {
        let c = font
            .outline(id)
            .map(|o| contours(&o.curves))
            .unwrap_or(Contours {
                points: Vec::new(),
                ends: Vec::new(),
            });
        max_points = max_points.max(c.points.len() as u16);
        max_contours = max_contours.max(c.ends.len() as u16);
        let (rec, bbox) = glyph_record(&c);
        records.push(rec);
        boxes.push(bbox);
        advances.push(font.h_advance_unscaled(id).round().clamp(0.0, 65535.0) as u16);
    }
    let n = records.len() as u16;
    let inked: Vec<&[i16; 4]> = boxes
        .iter()
        .zip(&records)
        .filter(|(_, r)| !r.is_empty())
        .map(|(b, _)| b)
        .collect();
    let fold =
        |k: usize, f: fn(i16, i16) -> i16, init: i16| inked.iter().map(|b| b[k]).fold(init, f);
    let bbox = if inked.is_empty() {
        [0, 0, 0, 0]
    } else {
        [
            fold(0, i16::min, i16::MAX),
            fold(1, i16::min, i16::MAX),
            fold(2, i16::max, i16::MIN),
            fold(3, i16::max, i16::MIN),
        ]
    };
    let ascent = round16(lf.ascent);
    let descent = -round16(lf.descent);

    // glyf and loca (long offsets).
    let mut glyf = Vec::new();
    let mut loca = Vec::with_capacity(4 * (n as usize + 1));
    for r in &records {
        loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
        glyf.extend_from_slice(r);
    }
    loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
    // hmtx: each glyph's advance and left side bearing.
    let mut hmtx = Vec::new();
    for (a, b) in advances.iter().zip(&boxes) {
        hmtx.extend_from_slice(&a.to_be_bytes());
        hmtx.extend_from_slice(&b[0].to_be_bytes());
    }
    let mut head = Vec::new();
    head.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    head.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // font revision
    head.extend_from_slice(&0u32.to_be_bytes()); // checksum adjustment, below
    head.extend_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // magic
    head.extend_from_slice(&0x000Bu16.to_be_bytes()); // flags
    head.extend_from_slice(&(upem as u16).to_be_bytes());
    head.extend_from_slice(&[0; 16]); // created, modified
    for v in bbox {
        head.extend_from_slice(&v.to_be_bytes());
    }
    head.extend_from_slice(&0u16.to_be_bytes()); // mac style
    head.extend_from_slice(&8u16.to_be_bytes()); // lowest ppem
    head.extend_from_slice(&2i16.to_be_bytes()); // direction hint
    head.extend_from_slice(&1i16.to_be_bytes()); // long loca
    head.extend_from_slice(&0i16.to_be_bytes()); // glyph data format
    let mut hhea = Vec::new();
    hhea.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea.extend_from_slice(&ascent.to_be_bytes());
    hhea.extend_from_slice(&descent.to_be_bytes());
    hhea.extend_from_slice(&round16(lf.line_gap).to_be_bytes());
    hhea.extend_from_slice(&advances.iter().max().copied().unwrap_or(0).to_be_bytes());
    hhea.extend_from_slice(&bbox[0].to_be_bytes()); // min left bearing
    hhea.extend_from_slice(&0i16.to_be_bytes()); // min right bearing
    hhea.extend_from_slice(&bbox[2].to_be_bytes()); // x max extent
    hhea.extend_from_slice(&1i16.to_be_bytes()); // caret slope rise
    hhea.extend_from_slice(&[0; 12]); // run, offset, reserved
    hhea.extend_from_slice(&0i16.to_be_bytes()); // metric data format
    hhea.extend_from_slice(&n.to_be_bytes()); // number of h metrics
    let mut maxp = Vec::new();
    maxp.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp.extend_from_slice(&n.to_be_bytes());
    maxp.extend_from_slice(&max_points.to_be_bytes());
    maxp.extend_from_slice(&max_contours.to_be_bytes());
    maxp.extend_from_slice(&[0, 0, 0, 0]); // composite points, contours
    maxp.extend_from_slice(&2u16.to_be_bytes()); // zones
    maxp.extend_from_slice(&[0; 18]); // the rest: no hinting
    let mut post = Vec::new();
    post.extend_from_slice(&0x0003_0000u32.to_be_bytes()); // no glyph names
    post.extend_from_slice(&[0; 28]);

    let tables: [(&[u8; 4], &[u8]); 7] = [
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca),
        (b"maxp", &maxp),
        (b"post", &post),
    ];
    let count = tables.len() as u16;
    let mut ttf = Vec::new();
    ttf.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    ttf.extend_from_slice(&count.to_be_bytes());
    // Search range, entry selector, range shift for 7 tables.
    ttf.extend_from_slice(&64u16.to_be_bytes());
    ttf.extend_from_slice(&2u16.to_be_bytes());
    ttf.extend_from_slice(&(count * 16 - 64).to_be_bytes());
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    let mut head_at = 0;
    for (tag, data) in tables {
        ttf.extend_from_slice(tag);
        ttf.extend_from_slice(&checksum(data).to_be_bytes());
        ttf.extend_from_slice(&(offset as u32).to_be_bytes());
        ttf.extend_from_slice(&(data.len() as u32).to_be_bytes());
        if tag == b"head" {
            head_at = 12 + 16 * tables.len() + body.len();
        }
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * tables.len() + body.len();
    }
    ttf.extend_from_slice(&body);
    let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&ttf));
    ttf[head_at + 8..head_at + 12].copy_from_slice(&adjust.to_be_bytes());

    let s = |v: i16| v as f32 * scale;
    Built {
        ttf,
        widths: advances[1..].iter().map(|&a| a as f32 * scale).collect(),
        bbox: [s(bbox[0]), s(bbox[1]), s(bbox[2]), s(bbox[3])],
        ascent: s(ascent),
        descent: s(descent),
    }
}

/// The ToUnicode CMap: each subset glyph back to the character it shows,
/// from the font's own character map.
pub fn to_unicode(fonts: &Fonts, face: FaceId, glyphs: &[GlyphId]) -> String {
    let mut chars: HashMap<GlyphId, char> = HashMap::new();
    for (id, c) in fonts.face(face).font.codepoint_ids() {
        chars
            .entry(id)
            .and_modify(|old| *old = (*old).min(c))
            .or_insert(c);
    }
    let pairs: Vec<(u16, char)> = glyphs
        .iter()
        .enumerate()
        .filter_map(|(k, id)| chars.get(id).map(|&c| (k as u16 + 1, c)))
        .collect();
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for block in pairs.chunks(100) {
        let _ = writeln!(out, "{} beginbfchar", block.len());
        for &(cid, c) in block {
            let mut units = [0u16; 2];
            let hex: String = c
                .encode_utf16(&mut units)
                .iter()
                .map(|u| format!("{u:04X}"))
                .collect();
            let _ = writeln!(out, "<{cid:04X}> <{hex}>");
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font file holding only an OS/2 table with `fs_type`.
    fn with_fs_type(fs_type: u16) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        f.extend_from_slice(&1u16.to_be_bytes());
        f.extend_from_slice(&[0; 6]);
        f.extend_from_slice(b"OS/2");
        f.extend_from_slice(&0u32.to_be_bytes());
        f.extend_from_slice(&28u32.to_be_bytes()); // offset
        f.extend_from_slice(&10u32.to_be_bytes());
        f.extend_from_slice(&[0; 8]);
        f.extend_from_slice(&fs_type.to_be_bytes());
        f
    }

    #[test]
    fn respects_embedding_permissions() {
        assert!(embeddable(&with_fs_type(0), 0)); // installable
        assert!(embeddable(&with_fs_type(0x0008), 0)); // editable
        assert!(!embeddable(&with_fs_type(0x0002), 0)); // restricted
        assert!(!embeddable(&with_fs_type(0x0200), 0)); // bitmap only
    }

    #[test]
    fn subset_fonts_parse_back() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let key = fonts.resolve("Times", false, false);
        let ids: Vec<GlyphId> = "Hello, world"
            .chars()
            .map(|c| fonts.glyph(key, c).1)
            .collect();
        let mut subset = Subset::default();
        for &id in &ids {
            subset.cid(id);
        }
        let built = build(&fonts, key.face, &subset.glyphs);
        // The font we made reads back with the same outlines and widths.
        let back = ab_glyph::FontRef::try_from_slice(&built.ttf).expect("a valid font");
        let lf = fonts.face(key.face);
        for (k, &id) in subset.glyphs.iter().enumerate() {
            let new = GlyphId(k as u16 + 1);
            let w = back.h_advance_unscaled(new);
            assert!((w - lf.font.h_advance_unscaled(id).round()).abs() < 0.5);
            let (a, b) = (back.outline(new), lf.font.outline(id));
            assert_eq!(a.is_some(), b.is_some());
            if let (Some(a), Some(b)) = (a, b) {
                assert!((a.bounds.min.x - b.bounds.min.x).abs() <= 1.0);
                assert!((a.bounds.max.y - b.bounds.max.y).abs() <= 1.0);
            }
        }
        let cmap = to_unicode(&fonts, key.face, &subset.glyphs);
        // 'H' is the first glyph used: subset glyph 1.
        assert!(cmap.contains("<0001> <0048>"), "{cmap}");
    }
}
