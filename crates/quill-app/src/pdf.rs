//! Writing pages as PDF, for printing and for File > Save as PDF.
//!
//! Pages are drawn by the same code that draws them on screen, onto a
//! `Surface` that writes PDF drawing operators instead of pixels. Text is
//! real PDF text in fonts made for the document from the outlines Quill
//! draws with (see `pdf_font`), each glyph placed where Quill's layout puts
//! it, so it prints sharply and can be searched and copied. A font whose
//! license forbids embedding is drawn instead as outlines, each glyph
//! written once as a form and reused.
//!
//! The file is written as it goes: each page's drawing is compressed into
//! the file as soon as the page is drawn, and each picture when it is
//! first met, so a long document never holds more than a page of drawing
//! operators. Before each page is drawn its paragraphs are given their
//! glyphs back, and the ones before are let go (see `docview::memory`).

use crate::canvas::{Rect, Surface};
use crate::docview::DocView;
use crate::pdf_font::{self, Subset};
use quill_layout::ab_glyph::Font as _;
use quill_layout::{Curve, FaceId, FaceKey, Fonts, GlyphCache, GlyphId, ITALIC_SHEAR};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

/// The PDF file being written: its bytes so far and where each object
/// starts. Objects are numbered as they are needed and written in any
/// order; the cross-reference table at the end puts them in order.
#[derive(Default)]
struct File {
    bytes: Vec<u8>,
    /// Each object's offset, by number less one.
    offsets: Vec<usize>,
}

impl File {
    /// A number for an object to be written later.
    fn alloc(&mut self) -> usize {
        self.offsets.push(0);
        self.offsets.len()
    }

    fn put(&mut self, num: usize, body: &[u8]) {
        self.offsets[num - 1] = self.bytes.len();
        self.bytes
            .extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        self.bytes.extend_from_slice(body);
        self.bytes.extend_from_slice(b"\nendobj\n");
    }
}

/// A glyph as text: its font, its number there and its width in
/// thousandths of the size.
type TextGlyph = (usize, u16, f32);

/// The glyphs and pictures used so far, and the file they go into.
#[derive(Default)]
struct Glyphs {
    index: HashMap<(FaceKey, GlyphId), usize>,
    /// Content (path operators) and bounding box of each form.
    forms: Vec<(String, [f32; 4])>,
    /// Pictures: their retained encoded data and the image's object number,
    /// already written. None when the picture could not be read.
    images: Vec<(Arc<[u8]>, Option<usize>)>,
    file: File,
    /// The fonts made for the document, by face, and the glyphs in each.
    subsets: Vec<(FaceId, Subset)>,
    /// Each face's font number, or None when it cannot be embedded.
    face_font: HashMap<FaceId, Option<usize>>,
    /// Each glyph used as text; None for glyphs with no outline.
    text: HashMap<(FaceId, GlyphId), Option<TextGlyph>>,
}

impl Glyphs {
    /// The glyph as text: its font, number and width, or None to draw it
    /// some other way (outlines) or not at all.
    fn text_glyph(
        &mut self,
        fonts: &Fonts,
        face: FaceId,
        id: GlyphId,
    ) -> Option<Option<TextGlyph>> {
        let fi = *self.face_font.entry(face).or_insert_with(|| {
            let lf = fonts.face(face);
            pdf_font::embeddable(lf.data, lf.index).then(|| {
                self.subsets.push((face, Subset::default()));
                self.subsets.len() - 1
            })
        });
        let fi = fi?;
        if let Some(&g) = self.text.get(&(face, id)) {
            return Some(g);
        }
        let lf = fonts.face(face);
        let g = lf.font.outline(id).map(|_| {
            let cid = self.subsets[fi].1.cid(id);
            let upem = lf.units_per_em.round().clamp(16.0, 16384.0);
            let w = lf.font.h_advance_unscaled(id).round().clamp(0.0, 65535.0) * 1000.0 / upem;
            (fi, cid, w)
        });
        self.text.insert((face, id), g);
        Some(g)
    }
}

/// The text object being written, so runs of glyphs share one text
/// operator and settings are only written when they change.
#[derive(Default)]
struct TextState {
    in_text: bool,
    /// Font number and size set with Tf.
    font: Option<(usize, f32)>,
    /// Fill (and stroke) color.
    color: Option<u32>,
    /// Fake bold (fill and stroke) at a size, or plain.
    mode: Option<Option<f32>>,
    /// The open run: baseline, shear and where the next glyph would go.
    run: Option<(f32, f32, f32)>,
}

/// An image object's bytes, and its soft mask's if it has transparency.
type ImageObject = (Vec<u8>, Option<Vec<u8>>);

/// A JPEG's component count, from its frame header.
fn jpeg_components(d: &[u8]) -> Option<u8> {
    let mut i = 2;
    while i + 9 < d.len() {
        if d[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = d[i + 1];
        if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            return d.get(i + 9).copied();
        }
        if m == 0xD8 || m == 0xFF || (0xD0..=0xD7).contains(&m) {
            i += if m == 0xFF { 1 } else { 2 };
            continue;
        }
        let len = u16::from_be_bytes([d[i + 2], d[i + 3]]) as usize;
        i += 2 + len;
    }
    None
}

/// A picture as a PDF image object, with a soft mask for transparency.
/// JPEGs in gray or RGB go in as they are; everything else is decoded.
fn image_object(pic: &quill_core::Picture) -> Option<ImageObject> {
    let (w, h) = pic.pixels;
    if pic.format == quill_core::ImageFormat::Jpeg
        && let Some(n @ (1 | 3)) = jpeg_components(&pic.data)
    {
        let cs = if n == 1 { "/DeviceGray" } else { "/DeviceRGB" };
        let mut v = format!(
            "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace {cs} /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            pic.data.len()
        )
        .into_bytes();
        v.extend_from_slice(&pic.data);
        v.extend_from_slice(b"\nendstream");
        return Some((v, None));
    }
    let (w, h, rgba) = crate::image::decode_rgba(&pic.data, pic.format)?;
    let (px, _) = rgba.as_chunks::<4>();
    let rgb: Vec<u8> = px.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let alpha: Vec<u8> = px.iter().map(|p| p[3]).collect();
    let dict = format!(
        "/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8"
    );
    let mask = alpha.iter().any(|&a| a < 255).then(|| {
        stream(
            &format!(
                "/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8"
            ),
            &alpha,
        )
    });
    Some((stream(&dict, &rgb), mask))
}

impl Glyphs {
    /// The form for a glyph, made on first use. None for blank glyphs.
    fn get(&mut self, fonts: &Fonts, face: FaceKey, id: GlyphId) -> Option<(usize, f32)> {
        let (upem, curves) = fonts.outline(face, id)?;
        if let Some(&n) = self.index.get(&(face, id)) {
            return Some((n, upem));
        }
        let mut path = String::new();
        let mut at: Option<(f32, f32)> = None;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut see = |p: (f32, f32)| {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        };
        let start = |path: &mut String, at: &mut Option<(f32, f32)>, p: (f32, f32)| {
            if *at != Some(p) {
                if at.is_some() {
                    path.push_str("h\n");
                }
                let _ = writeln!(path, "{} {} m", n(p.0), n(p.1));
            }
        };
        for c in &curves {
            match *c {
                Curve::Line(a, b) => {
                    start(&mut path, &mut at, a);
                    let _ = writeln!(path, "{} {} l", n(b.0), n(b.1));
                    see(a);
                    see(b);
                    at = Some(b);
                }
                Curve::Quad(a, q, b) => {
                    start(&mut path, &mut at, a);
                    // A quadratic as a cubic: controls two thirds of the
                    // way from each end to the quadratic's control.
                    let c1 = (a.0 + 2.0 / 3.0 * (q.0 - a.0), a.1 + 2.0 / 3.0 * (q.1 - a.1));
                    let c2 = (b.0 + 2.0 / 3.0 * (q.0 - b.0), b.1 + 2.0 / 3.0 * (q.1 - b.1));
                    let _ = writeln!(
                        path,
                        "{} {} {} {} {} {} c",
                        n(c1.0),
                        n(c1.1),
                        n(c2.0),
                        n(c2.1),
                        n(b.0),
                        n(b.1)
                    );
                    see(a);
                    see(q);
                    see(b);
                    at = Some(b);
                }
                Curve::Cubic(a, c1, c2, b) => {
                    start(&mut path, &mut at, a);
                    let _ = writeln!(
                        path,
                        "{} {} {} {} {} {} c",
                        n(c1.0),
                        n(c1.1),
                        n(c2.0),
                        n(c2.1),
                        n(b.0),
                        n(b.1)
                    );
                    see(a);
                    see(c1);
                    see(c2);
                    see(b);
                    at = Some(b);
                }
            }
        }
        // A glyph with no contours (a space) needs no form.
        at?;
        path.push_str("h\n");
        // Fake bold: stroke the outline as well as filling it.
        if face.fake_bold {
            let w = upem / 30.0;
            path = format!("{} w 1 j\n{path}B\n", n(w));
            x0 -= w;
            y0 -= w;
            x1 += w;
            y1 += w;
        } else {
            path.push_str("f\n");
        }
        let k = self.forms.len();
        self.forms.push((path, [x0, y0, x1, y1]));
        self.index.insert((face, id), k);
        Some((k, upem))
    }
}

/// A number for PDF: short, with no exponent.
fn n(v: f32) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

fn rgb(color: u32) -> String {
    let c = |sh: u32| n(((color >> sh) & 255) as f32 / 255.0);
    format!("{} {} {}", c(16), c(8), c(0))
}

/// One page being drawn: PDF operators, with y flipped from the screen's
/// downward points to PDF's upward ones.
struct Page<'a> {
    out: String,
    height: f32,
    glyphs: &'a mut Glyphs,
    text: TextState,
    /// The clips in force, innermost last, in the page's downward points.
    clips: Vec<Rect>,
}

impl Page<'_> {
    /// Whether a glyph on `baseline` is cut off by the clip: a line of the
    /// page before or after, drawn for its paragraph's sake. Leaving it out
    /// keeps hidden text out of searches and copies, and keeps descenders
    /// of the line above from peeking in.
    fn clipped_out(&self, baseline: f32) -> bool {
        self.clips
            .last()
            .is_some_and(|c| baseline < c.y - 0.01 || baseline > c.bottom() + 0.01)
    }

    /// Whether `r` is wholly outside the clip.
    fn hidden(&self, r: Rect) -> bool {
        self.clips.last().is_some_and(|c| {
            r.bottom() <= c.y || r.y >= c.bottom() || r.right() <= c.x || r.x >= c.right()
        })
    }

    /// Close the run of glyphs being written.
    fn end_run(&mut self) {
        if self.text.run.take().is_some() {
            self.out.push_str(">] TJ\n");
        }
    }

    /// Close the text object, before anything that is not text.
    fn end_text(&mut self) {
        self.end_run();
        if self.text.in_text {
            self.out.push_str("ET\n");
            self.text.in_text = false;
        }
    }

    /// The page's operators, with any text closed.
    fn finish(mut self) -> String {
        self.end_text();
        self.out
    }

    /// A glyph as an outline form, for fonts that cannot be embedded.
    #[allow(clippy::too_many_arguments)]
    fn outline_glyph(
        &mut self,
        fonts: &Fonts,
        face: FaceKey,
        id: GlyphId,
        size: f32,
        x: f32,
        baseline: f32,
        color: u32,
    ) {
        if self.clipped_out(baseline) {
            return;
        }
        let Some((k, upem)) = self.glyphs.get(fonts, face, id) else {
            return;
        };
        self.end_text();
        let s = size / upem;
        let shear = if face.fake_italic { ITALIC_SHEAR } else { 0.0 };
        let _ = writeln!(
            self.out,
            "q {} rg {} RG {} 0 {} {} {} {} cm /G{k} Do Q",
            rgb(color),
            rgb(color),
            n(s),
            n(s * shear),
            n(s),
            n(x),
            n(self.height - baseline)
        );
    }
}

impl Surface for Page<'_> {
    fn fill(&mut self, r: Rect, color: u32) {
        if r.w <= 0.0 || r.h <= 0.0 || self.hidden(r) {
            return;
        }
        self.end_text();
        // The fill color changes: text must set its own again.
        self.text.color = None;
        let _ = writeln!(
            self.out,
            "{} rg {} {} {} {} re f",
            rgb(color),
            n(r.x),
            n(self.height - r.y - r.h),
            n(r.w),
            n(r.h)
        );
    }

    fn hair(&self) -> f32 {
        0.6
    }

    fn glyph(
        &mut self,
        fonts: &Fonts,
        _cache: &mut GlyphCache,
        face: FaceKey,
        id: GlyphId,
        size: f32,
        x: f32,
        baseline: f32,
        color: u32,
    ) {
        if self.clipped_out(baseline) {
            return;
        }
        let (fi, cid, w) = match self.glyphs.text_glyph(fonts, face.face, id) {
            None => return self.outline_glyph(fonts, face, id, size, x, baseline, color),
            // Nothing to draw (a space).
            Some(None) => return,
            Some(Some(g)) => g,
        };
        let shear = if face.fake_italic { ITALIC_SHEAR } else { 0.0 };
        let y = self.height - baseline;
        if !self.text.in_text {
            self.out.push_str("BT\n");
            self.text.in_text = true;
        }
        if self.text.font != Some((fi, size)) {
            self.end_run();
            let _ = writeln!(self.out, "/F{fi} {} Tf", n(size));
            self.text.font = Some((fi, size));
        }
        if self.text.color != Some(color) {
            self.end_run();
            let _ = writeln!(self.out, "{} rg {} RG", rgb(color), rgb(color));
            self.text.color = Some(color);
        }
        // Fake bold: fill and stroke the glyphs, as the outlines did.
        let mode = face.fake_bold.then_some(size);
        if self.text.mode != Some(mode) {
            self.end_run();
            match mode {
                Some(size) => {
                    let _ = writeln!(self.out, "2 Tr {} w 1 j", n(size / 30.0));
                }
                None => self.out.push_str("0 Tr\n"),
            }
            self.text.mode = Some(mode);
        }
        // Carry on the run when the glyph is on its baseline, close to
        // where the last one ended; a small step is written in the run.
        match self.text.run {
            Some((ry, rs, next)) if ry == y && rs == shear && (x - next).abs() < size * 2.0 => {
                let dx = x - next;
                if dx.abs() > 0.0005 {
                    let _ = write!(self.out, "> {} <", n(-dx * 1000.0 / size));
                }
            }
            _ => {
                self.end_run();
                let _ = write!(self.out, "1 0 {} 1 {} {} Tm [<", n(shear), n(x), n(y));
            }
        }
        let _ = write!(self.out, "{cid:04X}");
        self.text.run = Some((y, shear, x + w * size / 1000.0));
    }

    fn glyph_art(
        &mut self,
        fonts: &Fonts,
        _cache: &mut GlyphCache,
        face: FaceKey,
        id: GlyphId,
        size: f32,
        x: f32,
        baseline: f32,
        color: u32,
    ) {
        self.outline_glyph(fonts, face, id, size, x, baseline, color);
    }

    fn push_clip(&mut self, r: Rect) {
        self.end_text();
        let inner = match self.clips.last() {
            Some(c) => {
                let (x0, y0) = (c.x.max(r.x), c.y.max(r.y));
                let (x1, y1) = (c.right().min(r.right()), c.bottom().min(r.bottom()));
                Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
            }
            None => r,
        };
        self.clips.push(inner);
        let _ = writeln!(
            self.out,
            "q {} {} {} {} re W n",
            n(r.x),
            n(self.height - r.y - r.h),
            n(r.w),
            n(r.h)
        );
    }

    fn pop_clip(&mut self) {
        self.end_text();
        self.clips.pop();
        self.out.push_str("Q\n");
        // Q brings back the settings from before the clip.
        self.text = TextState::default();
    }

    fn screen(&self) -> bool {
        false
    }

    fn picture(&mut self, pic: &quill_core::Picture, r: Rect) {
        if self.hidden(r) {
            return;
        }
        self.end_text();
        let k = match self
            .glyphs
            .images
            .iter()
            .position(|(data, _)| Arc::ptr_eq(data, &pic.data) || *data == pic.data)
        {
            Some(k) => k,
            None => {
                // Into the file now, so its bytes are not kept until the end.
                let num = image_object(pic).map(|(mut obj, mask)| {
                    let file = &mut self.glyphs.file;
                    let num = file.alloc();
                    if let Some(mask) = mask {
                        let mask_num = file.alloc();
                        // Point the image at its mask: add /SMask to the
                        // dictionary.
                        let at = obj.windows(2).position(|w| w == b"<<").unwrap_or(0) + 2;
                        obj.splice(at..at, format!(" /SMask {mask_num} 0 R").into_bytes());
                        file.put(mask_num, &mask);
                    }
                    file.put(num, &obj);
                    num
                });
                self.glyphs.images.push((pic.data.clone(), num));
                self.glyphs.images.len() - 1
            }
        };
        if self.glyphs.images[k].1.is_none() {
            return;
        }
        // The whole image, placed so that its shown part fills `r`.
        let src = crate::canvas::crop_fractions(pic);
        let fw = r.w / (src[2] - src[0]).max(0.001);
        let fh = r.h / (src[3] - src[1]).max(0.001);
        let fx = r.x - src[0] * fw;
        let fy = r.y - src[1] * fh;
        let _ = writeln!(
            self.out,
            "q {} {} {} {} re W n {} 0 0 {} {} {} cm /I{k} Do Q",
            n(r.x),
            n(self.height - r.y - r.h),
            n(r.w),
            n(r.h),
            n(fw),
            n(fh),
            n(fx),
            n(self.height - fy - fh)
        );
    }
}

/// Write pages `range` of the document as a PDF file's bytes. The view's
/// layout, pagination and headers must be up to date; each page's
/// paragraphs are laid out again as it is drawn.
pub fn write(
    view: &mut DocView,
    fonts: &mut Fonts,
    cache: &mut GlyphCache,
    range: std::ops::Range<usize>,
) -> Vec<u8> {
    write_with_progress(view, fonts, cache, range, |_, _| {})
}

/// Write a PDF while reporting each completed page. The callback is kept
/// outside the renderer so callers can turn it into UI events or ignore it.
pub fn write_with_progress(
    view: &mut DocView,
    fonts: &mut Fonts,
    cache: &mut GlyphCache,
    range: std::ops::Range<usize>,
    mut progress: impl FnMut(usize, usize),
) -> Vec<u8> {
    let (pw, ph) = view.page_size();
    let mut g = Glyphs::default();
    g.file.bytes = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    // 1 catalog, 2 page tree, 3 resources: written last, numbered first.
    let (catalog, tree, resources) = (g.file.alloc(), g.file.alloc(), g.file.alloc());
    let mut pages: Vec<Option<usize>> = range.filter(|&k| k < view.pages.len()).map(Some).collect();
    if pages.is_empty() {
        pages.push(None);
    }
    let mut kids = Vec::with_capacity(pages.len());
    let page_count = pages.len();
    for (page_index, k) in pages.into_iter().enumerate() {
        let mut page = Page {
            out: String::new(),
            height: ph,
            glyphs: &mut g,
            text: TextState::default(),
            clips: Vec::new(),
        };
        if let Some(k) = k {
            view.prepare_pages(fonts, &[k]);
            view.draw_page(&mut page, fonts, cache, false, k, 0.0, 0.0);
        }
        let content = page.finish();
        let (page_num, content_num) = (g.file.alloc(), g.file.alloc());
        g.file.put(content_num, &stream("", content.as_bytes()));
        g.file.put(
            page_num,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 {} {}] /Resources {resources} 0 R /Contents {content_num} 0 R >>",
                n(pw),
                n(ph)
            )
            .as_bytes(),
        );
        kids.push(format!("{page_num} 0 R"));
        progress(page_index + 1, page_count);
    }

    let mut xobjects = String::new();
    let forms = std::mem::take(&mut g.forms);
    for (k, (path, b)) in forms.iter().enumerate() {
        let num = g.file.alloc();
        let dict = format!(
            "/Type /XObject /Subtype /Form /BBox [{} {} {} {}]",
            n(b[0]),
            n(b[1]),
            n(b[2]),
            n(b[3])
        );
        g.file.put(num, &stream(&dict, path.as_bytes()));
        let _ = write!(xobjects, "/G{k} {num} 0 R ");
    }
    for (k, (_, num)) in g.images.iter().enumerate() {
        if let Some(num) = num {
            let _ = write!(xobjects, "/I{k} {num} 0 R ");
        }
    }
    // The fonts, now every glyph they need is known.
    let mut font_res = String::new();
    let subsets = std::mem::take(&mut g.subsets);
    for (k, (face, subset)) in subsets.iter().enumerate() {
        let num = write_font(&mut g.file, fonts, k, *face, subset);
        let _ = write!(font_res, "/F{k} {num} 0 R ");
    }
    let file = &mut g.file;
    file.put(
        resources,
        format!("<< /XObject << {xobjects}>> /Font << {font_res}>> >>").as_bytes(),
    );
    file.put(
        tree,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            kids.len()
        )
        .as_bytes(),
    );
    file.put(
        catalog,
        format!("<< /Type /Catalog /Pages {tree} 0 R >>").as_bytes(),
    );

    let count = file.offsets.len();
    let xref = file.bytes.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", count + 1);
    for o in &file.offsets {
        let _ = writeln!(table, "{o:010} 00000 n ");
    }
    let _ = write!(
        table,
        "trailer\n<< /Size {} /Root {catalog} 0 R /Info << /Producer (Quill) >> >>\nstartxref\n{xref}\n%%EOF\n",
        count + 1
    );
    file.bytes.extend_from_slice(table.as_bytes());
    std::mem::take(&mut file.bytes)
}

/// Write font `k` (face `face`, holding `subset`) as a Type 0 font with a
/// TrueType font file and a ToUnicode map. Returns its object number.
fn write_font(file: &mut File, fonts: &Fonts, k: usize, face: FaceId, subset: &Subset) -> usize {
    let built = pdf_font::build(fonts, face, &subset.glyphs);
    // A subset font's name: six capitals of its own, then the face's.
    let mut tag = String::new();
    let mut v = k;
    for _ in 0..6 {
        tag.push((b'A' + (v % 26) as u8) as char);
        v /= 26;
    }
    let base: String = fonts
        .face(face)
        .name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let name = format!("{tag}+{}", if base.is_empty() { "Font" } else { &base });
    let font_file = file.alloc();
    file.put(
        font_file,
        &stream(&format!("/Length1 {}", built.ttf.len()), &built.ttf),
    );
    let desc = file.alloc();
    let b = built.bbox;
    file.put(
        desc,
        format!(
            "<< /Type /FontDescriptor /FontName /{name} /Flags 4 /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {font_file} 0 R >>",
            n(b[0]),
            n(b[1]),
            n(b[2]),
            n(b[3]),
            n(built.ascent),
            n(built.descent),
            n(built.ascent)
        )
        .as_bytes(),
    );
    let widths: Vec<String> = built.widths.iter().map(|&w| n(w)).collect();
    let cid_font = file.alloc();
    file.put(
        cid_font,
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {desc} 0 R /DW 0 /W [1 [{}]] /CIDToGIDMap /Identity >>",
            widths.join(" ")
        )
        .as_bytes(),
    );
    let to_unicode = file.alloc();
    let cmap = pdf_font::to_unicode(fonts, face, &subset.glyphs);
    file.put(to_unicode, &stream("", cmap.as_bytes()));
    let font = file.alloc();
    file.put(
        font,
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [{cid_font} 0 R] /ToUnicode {to_unicode} 0 R >>"
        )
        .as_bytes(),
    );
    font
}

/// A compressed stream object with extra dictionary entries `dict`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let packed = miniz_oxide::deflate::compress_to_vec_zlib(data, 6);
    let mut v = format!(
        "<< {dict} /Length {} /Filter /FlateDecode >>\nstream\n",
        packed.len()
    )
    .into_bytes();
    v.extend_from_slice(&packed);
    v.extend_from_slice(b"\nendstream");
    v
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn numbers_are_short() {
        assert_eq!(n(1.0), "1");
        assert_eq!(n(0.5), "0.5");
        assert_eq!(n(-0.0001), "0");
        assert_eq!(n(612.0), "612");
        assert_eq!(n(1.23456), "1.235");
    }

    #[test]
    fn writes_a_wellformed_pdf() {
        let Some(fonts) = test_fonts() else { return };
        let mut fonts = fonts;
        let mut doc = quill_core::Document::new();
        let chp = doc.main.paras[0].mark_chp;
        doc.main.paras[0].insert(0, "Hello, PDF.", chp);
        let mut v = DocView::new(doc, "Test".into());
        v.headers_wanted = true;
        v.update_layout(&mut fonts);
        let mut cache = GlyphCache::default();
        let pdf = write(&mut v, &mut fonts, &mut cache, 0..1);
        let find = |needle: &[u8]| pdf.windows(needle.len()).position(|w| w == needle);
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(find(b"/MediaBox [0 0 612 792]").is_some());
        assert!(find(b"/Subtype /Type0").is_some(), "a font was written");
        assert!(find(b"/FontFile2").is_some(), "with its font file");
        assert!(find(b"/ToUnicode").is_some(), "and its character map");
        assert!(pdf.ends_with(b"%%EOF\n"));
        // startxref points at the table, and the table at the objects.
        let xref = find(b"\nxref\n").unwrap() + 1;
        let sx = find(b"startxref\n").unwrap() + 10;
        let start: usize = std::str::from_utf8(&pdf[sx..])
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(start, xref);
        let table = std::str::from_utf8(&pdf[xref..]).unwrap();
        for (i, line) in table.lines().skip(3).take(3).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            let head = format!("{} 0 obj", i + 1);
            assert!(pdf[off..].starts_with(head.as_bytes()), "object {}", i + 1);
        }
    }

    #[test]
    fn repeated_picture_bytes_make_one_pdf_image() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let mut doc = quill_core::Document::new();
        let chp = doc.main.paras[0].mark_chp;
        let bytes = crate::png::encode(2, 1, &[0x112233, 0x445566]);
        let first = quill_core::Picture::from_bytes(bytes.clone()).unwrap();
        let second = quill_core::Picture::from_bytes(bytes).unwrap();
        let at = quill_core::Position::new(0, 0);
        let at = doc.insert_picture(quill_core::StoryId::Main, at, first, chp);
        doc.insert_picture(quill_core::StoryId::Main, at, second, chp);
        let mut view = DocView::new(doc, "Pictures".into());
        view.update_layout(&mut fonts);
        let mut cache = GlyphCache::default();
        let pdf = write(&mut view, &mut fonts, &mut cache, 0..1);
        assert_eq!(
            pdf.windows(b"/Subtype /Image".len())
                .filter(|w| *w == b"/Subtype /Image")
                .count(),
            1
        );
    }

    #[test]
    fn text_outside_the_clip_is_left_out() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let key = fonts.resolve("Times", false, false);
        let id = fonts.glyph(key, 'A').1;
        let mut g = Glyphs::default();
        let mut cache = GlyphCache::default();
        let mut page = Page {
            out: String::new(),
            height: 792.0,
            glyphs: &mut g,
            text: TextState::default(),
            clips: Vec::new(),
        };
        page.push_clip(Rect::new(0.0, 100.0, 612.0, 100.0));
        // A line of the page before, its baseline above the clip.
        page.glyph(&fonts, &mut cache, key, id, 12.0, 72.0, 95.0, 0);
        assert!(!page.out.contains("Tm"), "{}", page.out);
        // And one inside it.
        page.glyph(&fonts, &mut cache, key, id, 12.0, 72.0, 150.0, 0);
        assert!(page.out.contains("Tm [<0001"), "{}", page.out);
        page.pop_clip();
        let out = page.finish();
        assert!(out.contains("TJ") && out.trim_end().ends_with('Q'));
    }

    pub fn test_fonts() -> Option<Fonts> {
        let f = Fonts::new(quill_layout::Catalog::scan_system());
        (!f.family_names().is_empty()).then_some(f)
    }
}
