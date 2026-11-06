//! Quill's icons: flat vector pictures that stay sharp at any scale.
//!
//! An icon is a few lines of text, one shape per line, on a grid of points:
//! (3, 4) is the center of the square in column 3, row 4, so a one-point
//! line through it fills that square exactly at 1x, and a fill edge sits
//! half a point out (1.5 is the left edge of column 2). Lines are one point
//! wide unless they say otherwise, snapped to whole device pixels, with
//! round ends and joins once they are two device pixels or more.
//!
//! Shapes, painted in order. T is a tone: `k` ink, `s` soft gray, `B` blue,
//! `b` pale blue, `R` red, `G` green, `Y` amber, `y` pale amber, `W` white,
//! `#` black, `D` dark gray, `m` gray, `L` the background, `-` none. A
//! stroke tone can carry a weight in points, as in `k/1.5`.
//!
//! ```text
//! poly FILL STROKE x y x y ...  closed outline
//! rrect FILL STROKE x0 y0 x1 y1 r   rectangle through these centers,
//!                               corners rounded to radius r
//! line T x y x y ...            open line
//! dash T x0 y0 x1 y1 on off     dashed line, dashes and gaps in points
//! arc T cx cy r a0 a1           part of a circle, angles in degrees
//!                               clockwise from the right
//! curve T x0 y0 cx cy x1 y1     quadratic curve
//! rect T x0 y0 x1 y1            solid block of whole pixels, inclusive
//! box T x0 y0 x1 y1             frame around whole pixels, drawn inward
//! bar T x0 x1 y                 a line of "text" across pixels x0..=x1
//! dot T x y r                   filled circle
//! ring T x y rx ry              outlined ellipse
//! clip x0 y0 x1 y1              only paint inside these whole pixels
//! clip                          paint anywhere again
//! text T font size bold x base string   letters, drawn by the caller with
//!                               x and baseline on pixel edges
//! ```

use ab_glyph_rasterizer::{Rasterizer, point};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::canvas::{Canvas, Label};

/// Letters an icon wants drawn over its picture, in points from the icon's
/// top left corner.
#[derive(Clone, Copy, Debug)]
pub struct Text<'a> {
    pub color: u32,
    pub font: &'a str,
    pub size: f32,
    pub bold: bool,
    pub x: f32,
    pub base: f32,
    pub text: &'a str,
}

/// An icon rendered for one scale: premultiplied 0xAARRGGBB pixels.
pub struct Image<'a> {
    pub w: u32,
    pub px: Vec<u32>,
    pub texts: Vec<Text<'a>>,
}

/// The icons' usual line and lettering color.
pub const INK: u32 = 0x3A3A3A;

fn tone(c: &str, face: u32) -> Option<u32> {
    Some(match c.as_bytes().first()? {
        b'k' => INK,
        b's' => 0x8E8E8E,
        b'B' => 0x2F6FD0,
        b'b' => 0xD3E3FA,
        b'R' => 0xD5463A,
        b'G' => 0x2E9A57,
        b'Y' => 0xE6A532,
        b'y' => 0xFBE6B0,
        b'#' => 0x000000,
        b'D' => 0x404040,
        b'm' => 0x808080,
        b'W' => 0xFFFFFF,
        b'L' => face,
        _ => return None,
    })
}

/// The standard line, in device pixels, for `n` device pixels per point.
fn weight(n: f32, w: Option<f32>) -> f32 {
    (w.unwrap_or(1.0) * n).round().max(1.0)
}

/// A spare border around the icon so strokes near the edge rasterize whole.
const PAD: f32 = 2.0;

struct Painter {
    n: f32,
    /// The padding in whole device pixels.
    pad: f32,
    /// Raster size, padding included.
    w: usize,
    h: usize,
    /// Premultiplied RGBA.
    buf: Vec<[f32; 4]>,
    clip: (usize, usize, usize, usize),
}

type Pt = (f32, f32);

impl Painter {
    /// Device position of grid coordinate `v`.
    fn dev(&self, v: f32) -> f32 {
        (v + 0.5) * self.n + self.pad
    }

    /// Like `dev`, but put a line of `tw` device pixels on whole pixels.
    fn snap(&self, v: f32, tw: f32) -> f32 {
        let d = self.dev(v);
        if tw as i32 % 2 == 1 {
            d.floor() + 0.5
        } else {
            d.round()
        }
    }

    fn fill(&mut self, polys: &[Vec<Pt>], color: u32) {
        let mut r = Rasterizer::new(self.w, self.h);
        let (mw, mh) = (self.w as f32 - 0.01, self.h as f32 - 0.01);
        let cl = |p: Pt| point(p.0.clamp(0.0, mw), p.1.clamp(0.0, mh));
        for poly in polys {
            for i in 0..poly.len() {
                r.draw_line(cl(poly[i]), cl(poly[(i + 1) % poly.len()]));
            }
        }
        let c = [
            ((color >> 16) & 255) as f32 / 255.0,
            ((color >> 8) & 255) as f32 / 255.0,
            (color & 255) as f32 / 255.0,
            1.0,
        ];
        let (x0, y0, x1, y1) = self.clip;
        let w = self.w;
        r.for_each_pixel_2d(|x, y, a| {
            let (x, y) = (x as usize, y as usize);
            if a <= 0.0 || x < x0 || x >= x1 || y < y0 || y >= y1 {
                return;
            }
            let a = a.min(1.0);
            let d = &mut self.buf[y * w + x];
            for k in 0..4 {
                d[k] = c[k] * a + d[k] * (1.0 - a);
            }
        });
    }

    /// Outline a path `tw` device pixels wide. One-pixel lines get square
    /// ends so they sit crisply on the pixels; wider ones get round ends
    /// and joins.
    fn stroke(&mut self, pts: &[Pt], closed: bool, tw: f32, color: u32) {
        if pts.len() < 2 {
            return;
        }
        let round = tw >= 2.0;
        let mut quads = Vec::new();
        let segs = if closed { pts.len() } else { pts.len() - 1 };
        let h = tw / 2.0;
        for i in 0..segs {
            let (p0, p1) = (pts[i], pts[(i + 1) % pts.len()]);
            let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-3 {
                continue;
            }
            let (ux, uy) = (dx / len * h, dy / len * h);
            let (ex, ey) = if round { (0.0, 0.0) } else { (ux, uy) };
            let (a, b) = ((p0.0 - ex, p0.1 - ey), (p1.0 + ex, p1.1 + ey));
            quads.push(vec![
                (a.0 + uy, a.1 - ux),
                (b.0 + uy, b.1 - ux),
                (b.0 - uy, b.1 + ux),
                (a.0 - uy, a.1 + ux),
            ]);
        }
        if round {
            // Circles turn the same way as the quads, so where they overlap
            // the coverage adds up rather than canceling.
            for &(cx, cy) in pts {
                quads.push(
                    (0..24)
                        .map(|i| {
                            let t = i as f32 / 24.0 * std::f32::consts::TAU;
                            (cx + h * t.cos(), cy + h * t.sin())
                        })
                        .collect(),
                );
            }
        }
        self.fill(&quads, color);
    }

    /// A rectangle through grid centers with rounded corners, in device
    /// pixels, its straight edges snapped for a line `tw` wide.
    fn rounded(&self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, tw: f32) -> Vec<Pt> {
        let (l, t) = (self.snap(x0, tw), self.snap(y0, tw));
        let (rt, b) = (self.snap(x1, tw), self.snap(y1, tw));
        let r = (r * self.n).min((rt - l) / 2.0).min((b - t) / 2.0).max(0.0);
        let mut pts = Vec::new();
        for (cx, cy, from) in [
            (l + r, t + r, 180.0f32),
            (rt - r, t + r, 270.0),
            (rt - r, b - r, 0.0),
            (l + r, b - r, 90.0),
        ] {
            for i in 0..=6 {
                let a = (from + 15.0 * i as f32).to_radians();
                pts.push((cx + r * a.cos(), cy + r * a.sin()));
            }
        }
        pts
    }

    fn ellipse(&self, cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<Pt> {
        let (cx, cy, rx, ry) = (self.dev(cx), self.dev(cy), rx * self.n, ry * self.n);
        (0..48)
            .map(|i| {
                let t = i as f32 / 48.0 * std::f32::consts::TAU;
                (cx + rx * t.cos(), cy + ry * t.sin())
            })
            .collect()
    }

    fn whole(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Pt> {
        let (l, t) = (self.dev(x0 - 0.5), self.dev(y0 - 0.5));
        let (r, b) = (self.dev(x1 + 0.5), self.dev(y1 + 0.5));
        vec![(l, t), (r, t), (r, b), (l, b)]
    }
}

fn nums(args: &[&str]) -> Vec<f32> {
    args.iter().filter_map(|a| a.parse().ok()).collect()
}

fn pairs(v: &[f32]) -> Vec<Pt> {
    v.as_chunks::<2>().0.iter().map(|&[x, y]| (x, y)).collect()
}

/// Split a tone like `#/1.4` into its color and weight.
fn toned(t: &str, face: u32) -> (Option<u32>, Option<f32>) {
    let mut it = t.splitn(2, '/');
    let c = tone(it.next().unwrap_or(""), face);
    (c, it.next().and_then(|w| w.parse().ok()))
}

/// The device size of `w` points at `n` device pixels a point.
pub fn device_len(w: u32, n: f32) -> u32 {
    (w as f32 * n).round() as u32
}

/// Render `spec` on a grid `w` by `h` old pixels at `n` device pixels per
/// pixel, with `face` for the `L` tone. `n` may be fractional, as at a
/// 1.25x interface; lines still snap to whole device pixels.
pub fn render(spec: &str, w: u32, h: u32, n: f32, face: u32) -> Image<'_> {
    let nf = n;
    let pad = (PAD * nf).ceil() as usize;
    let (ow, oh) = (device_len(w, n), device_len(h, n));
    let (rw, rh) = (ow as usize + 2 * pad, oh as usize + 2 * pad);
    let mut p = Painter {
        n: nf,
        pad: pad as f32,
        w: rw,
        h: rh,
        buf: vec![[0.0; 4]; rw * rh],
        clip: (0, 0, rw, rh),
    };
    let mut texts = Vec::new();
    for line in spec.lines() {
        let line = line.trim();
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some((&op, args)) = words.split_first() else {
            continue;
        };
        let (c, wt) = args.first().map_or((None, None), |t| toned(t, face));
        let tw = weight(nf, wt);
        match op {
            "poly" if args.len() >= 2 => {
                let v = nums(&args[2..]);
                let (stroke, swt) = toned(args[1], face);
                let tw = weight(nf, swt);
                let pts: Vec<Pt> = pairs(&v)
                    .into_iter()
                    .map(|(x, y)| {
                        if stroke.is_some() {
                            (p.snap(x, tw), p.snap(y, tw))
                        } else {
                            (p.dev(x), p.dev(y))
                        }
                    })
                    .collect();
                if let Some(f) = c {
                    p.fill(std::slice::from_ref(&pts), f);
                }
                if let Some(s) = stroke {
                    p.stroke(&pts, true, tw, s);
                }
            }
            "line" => {
                let pts: Vec<Pt> = pairs(&nums(&args[1..]))
                    .into_iter()
                    .map(|(x, y)| (p.snap(x, tw), p.snap(y, tw)))
                    .collect();
                if let (Some(c), true) = (c, pts.len() >= 2) {
                    p.stroke(&pts, false, tw, c);
                }
            }
            "rrect" if args.len() >= 2 => {
                let (stroke, swt) = toned(args[1], face);
                let tw = weight(nf, swt);
                if let [x0, y0, x1, y1, r] = nums(&args[2..]).as_slice() {
                    let pts = p.rounded(*x0, *y0, *x1, *y1, *r, tw);
                    if let Some(f) = c {
                        p.fill(std::slice::from_ref(&pts), f);
                    }
                    if let Some(s) = stroke {
                        p.stroke(&pts, true, tw, s);
                    }
                }
            }
            "dash" => {
                if let (Some(c), [x0, y0, x1, y1, on, off]) = (c, nums(&args[1..]).as_slice()) {
                    let (dx, dy) = (x1 - x0, y1 - y0);
                    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
                    let (ux, uy) = (dx / len, dy / len);
                    let h = tw / 2.0;
                    let (nx, ny) = (uy * h, -ux * h);
                    let mut quads = Vec::new();
                    let mut d = 0.0;
                    while d <= len + 0.01 {
                        // Each dash runs from the middle of its first square
                        // to the middle of its last, plus half a point each
                        // way, so a one-point dash fills one square.
                        let e = (d + on - 1.0).min(len);
                        let end = |k: f32| (x0 + ux * k, y0 + uy * k);
                        let (a, b) = (end(d - 0.5), end(e + 0.5));
                        let mut a = (p.dev(a.0), p.dev(a.1));
                        let mut b = (p.dev(b.0), p.dev(b.1));
                        // Lines along the grid stay on whole pixels.
                        if dx == 0.0 {
                            (a.0, b.0) = (p.snap(*x0, tw), p.snap(*x0, tw));
                        }
                        if dy == 0.0 {
                            (a.1, b.1) = (p.snap(*y0, tw), p.snap(*y0, tw));
                        }
                        quads.push(vec![
                            (a.0 + nx, a.1 + ny),
                            (b.0 + nx, b.1 + ny),
                            (b.0 - nx, b.1 - ny),
                            (a.0 - nx, a.1 - ny),
                        ]);
                        d += (on + off).max(1.0);
                    }
                    p.fill(&quads, c);
                }
            }
            "arc" => {
                if let (Some(c), [cx, cy, r, a0, a1]) = (c, nums(&args[1..]).as_slice()) {
                    let steps = ((a1 - a0).abs() / 10.0).ceil().max(2.0) as usize;
                    let pts: Vec<Pt> = (0..=steps)
                        .map(|i| {
                            let a = (a0 + (a1 - a0) * i as f32 / steps as f32).to_radians();
                            (p.dev(cx + r * a.cos()), p.dev(cy + r * a.sin()))
                        })
                        .collect();
                    p.stroke(&pts, false, tw, c);
                }
            }
            "curve" => {
                let v = nums(&args[1..]);
                if let (Some(c), [x0, y0, cx, cy, x1, y1]) = (c, v.as_slice()) {
                    let pts: Vec<Pt> = (0..=12)
                        .map(|i| {
                            let t = i as f32 / 12.0;
                            let u = 1.0 - t;
                            let x = u * u * x0 + 2.0 * u * t * cx + t * t * x1;
                            let y = u * u * y0 + 2.0 * u * t * cy + t * t * y1;
                            (p.dev(x), p.dev(y))
                        })
                        .collect();
                    p.stroke(&pts, false, tw, c);
                }
            }
            "rect" => {
                if let (Some(c), [x0, y0, x1, y1]) = (c, nums(&args[1..]).as_slice()) {
                    let r = p.whole(*x0, *y0, *x1, *y1);
                    p.fill(&[r], c);
                }
            }
            "box" => {
                if let (Some(c), [x0, y0, x1, y1]) = (c, nums(&args[1..]).as_slice()) {
                    let o = p.whole(*x0, *y0, *x1, *y1);
                    let (l, t, r, b) = (o[0].0, o[0].1, o[2].0, o[2].1);
                    let side =
                        |a: f32, b: f32, c: f32, d: f32| vec![(a, b), (c, b), (c, d), (a, d)];
                    p.fill(
                        &[
                            side(l, t, r, t + tw),
                            side(l, b - tw, r, b),
                            side(l, t + tw, l + tw, b - tw),
                            side(r - tw, t + tw, r, b - tw),
                        ],
                        c,
                    );
                }
            }
            "bar" => {
                if let (Some(c), [x0, x1, y]) = (c, nums(&args[1..]).as_slice()) {
                    let (l, r) = (p.dev(x0 - 0.5), p.dev(x1 + 0.5));
                    let mid = p.snap(*y, tw);
                    let (t, b) = (mid - tw / 2.0, mid + tw / 2.0);
                    p.fill(&[vec![(l, t), (r, t), (r, b), (l, b)]], c);
                }
            }
            "dot" => {
                if let (Some(c), [x, y, r]) = (c, nums(&args[1..]).as_slice()) {
                    let e = p.ellipse(*x, *y, *r, *r);
                    p.fill(&[e], c);
                }
            }
            "ring" => {
                if let (Some(c), [x, y, rx, ry]) = (c, nums(&args[1..]).as_slice()) {
                    let e = p.ellipse(*x, *y, *rx, *ry);
                    p.stroke(&e, true, tw, c);
                }
            }
            "clip" => {
                p.clip = match nums(args).as_slice() {
                    [x0, y0, x1, y1] => {
                        let r = p.whole(*x0, *y0, *x1, *y1);
                        (
                            r[0].0 as usize,
                            r[0].1 as usize,
                            r[2].0 as usize,
                            r[2].1 as usize,
                        )
                    }
                    _ => (0, 0, rw, rh),
                };
            }
            "text" if args.len() >= 7 => {
                if let (Some(color), Ok(size), Ok(x), Ok(base)) =
                    (c, args[2].parse(), args[4].parse(), args[5].parse())
                {
                    // The string is everything after the sixth argument.
                    let mut rest = line;
                    for _ in 0..7 {
                        rest = rest.trim_start();
                        rest = &rest[rest.find(char::is_whitespace).unwrap_or(rest.len())..];
                    }
                    texts.push(Text {
                        color,
                        font: args[1],
                        size,
                        bold: args[3] == "1",
                        x,
                        base,
                        text: rest.trim(),
                    });
                }
            }
            _ => {}
        }
    }
    // Drop the padding and pack the pixels.
    let mut px = Vec::with_capacity((ow * oh) as usize);
    for y in 0..oh as usize {
        for x in 0..ow as usize {
            let [r, g, b, a] = p.buf[(y + pad) * rw + x + pad];
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
            px.push(q(a) << 24 | q(r) << 16 | q(g) << 8 | q(b));
        }
    }
    Image { w: ow, px, texts }
}

/// `spec` rendered on a `w` x `h` grid at `n` device pixels a point, kept
/// for the next time it is asked for.
pub fn cached(spec: &'static str, w: u32, h: u32, n: f32) -> Rc<Image<'static>> {
    thread_local! {
        static CACHE: RefCell<HashMap<(usize, u32), Rc<Image<'static>>>> =
            RefCell::new(HashMap::new());
    }
    // A spec is a static string, so its address names it.
    let key = (spec.as_ptr() as usize, n.to_bits());
    CACHE.with(|c| {
        c.borrow_mut()
            .entry(key)
            .or_insert_with(|| Rc::new(render(spec, w, h, n, 0)))
            .clone()
    })
}

/// Draw a rendered icon with its top left corner at (`x`, `y`) in points,
/// and any letters it carries. `alpha` below 255 fades a disabled icon,
/// its letters toward `back`.
#[allow(clippy::too_many_arguments)]
pub fn paint(c: &mut Canvas, label: &mut Label, im: &Image, x: f32, y: f32, alpha: u8, back: u32) {
    let k = c.scale;
    c.image(
        &im.px,
        im.w,
        (x * k).round() as i32,
        (y * k).round() as i32,
        alpha as u32,
    );
    let t = alpha as f32 / 255.0;
    for tx in &im.texts {
        let ch = |s: u32| {
            let (a, b) = (((tx.color >> s) & 255) as f32, ((back >> s) & 255) as f32);
            ((b + (a - b) * t).round() as u32) << s
        };
        let ink = ch(16) | ch(8) | ch(0);
        label.draw(
            c,
            tx.font,
            tx.size,
            tx.bold,
            x + tx.x,
            y + tx.base,
            tx.text,
            ink,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_fills_whole_pixels() {
        let im = render("rect # 1 1 1 1", 3, 3, 3.0, 0xDDDDDD);
        assert_eq!(im.w, 9);
        // Pixel (1, 1) covers device pixels 3..6 in both directions.
        assert_eq!(im.px[4 * 9 + 4], 0xFF00_0000);
        assert_eq!(im.px[2 * 9 + 2], 0);
        assert_eq!(im.px[6 * 9 + 6], 0);
    }

    #[test]
    fn lines_land_on_whole_device_pixels() {
        // One point is three device pixels at 3x, filling row 1 exactly.
        let im = render("line # 0 1 2 1", 3, 3, 3.0, 0);
        let col: Vec<u32> = (0..9).map(|y| im.px[y * 9 + 4] >> 24).collect();
        assert_eq!(col, [0, 0, 0, 255, 255, 255, 0, 0, 0]);
        assert!(col.iter().all(|&a| a == 0 || a == 255));
    }

    #[test]
    fn round_joins_fill_the_corner() {
        // An L at 3x: three device pixels wide, the corner solid.
        let im = render("line k 1 4 1 1 4 1", 6, 6, 3.0, 0);
        let at = |x: u32, y: u32| im.px[(y * im.w + x) as usize] >> 24;
        assert_eq!(at(4, 4), 255);
        assert_eq!(at(4, 10), 255);
        assert_eq!(at(10, 4), 255);
    }

    #[test]
    fn rounded_rects_are_crisp_along_their_sides() {
        let im = render("rrect W k 1 1 8 8 2", 10, 10, 1.0, 0);
        // The left side runs down column 1, fully inked, away from the corners.
        for y in 4..6 {
            assert_eq!(im.px[(y * 10 + 1) as usize], 0xFF00_0000 | INK);
        }
        // Inside is white.
        assert_eq!(im.px[(5 * 10 + 5) as usize], 0xFFFF_FFFF);
    }

    #[test]
    fn one_point_dashes_fill_single_squares() {
        let im = render("dash k 0 1 6 1 1 1", 7, 3, 1.0, 0);
        let row: Vec<u32> = (0..7).map(|x| im.px[7 + x] >> 24).collect();
        assert_eq!(row, [255, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn text_keeps_the_rest_of_the_line() {
        let im = render("text # Helvetica 9 1 2 14 B b", 19, 18, 2.0, 0);
        assert_eq!(im.texts.len(), 1);
        assert_eq!(im.texts[0].text, "B b");
        assert!(im.texts[0].bold);
    }

    #[test]
    fn fractional_scales_size_and_snap_lines() {
        // 19 x 18 points at 1.25x is 24 x 23 device pixels, and a one-point
        // line is one whole pixel wide.
        let im = render("line # 0 4 18 4", 19, 18, 1.25, 0);
        assert_eq!(im.w, 24);
        assert_eq!(im.px.len(), 24 * 23);
        let col: Vec<u32> = (0..23).map(|y| im.px[y * 24 + 10] >> 24).collect();
        assert_eq!(col.iter().filter(|&&a| a == 255).count(), 1);
        assert!(col.iter().all(|&a| a == 0 || a == 255));
    }
}
