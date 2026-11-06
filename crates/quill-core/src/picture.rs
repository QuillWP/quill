//! Pictures: the objects a paragraph can hold. A picture is one character
//! (U+FFFC) whose run names an entry in `Document::objects`. Live entries do
//! not change, so undo can always find the one a paragraph used; scaling or
//! cropping a picture adds a new entry sharing the same data. Once bounded
//! undo history no longer reaches an entry, its slot may be reclaimed.

use crate::props::Twips;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
        }
    }
}

/// Cropping, in 1/100000ths of the picture's natural width or height, as
/// .docx keeps it. Negative values add white space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Crop {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub const CROP_WHOLE: i32 = 100_000;

#[derive(Clone, Debug)]
pub struct Picture {
    /// The file's bytes, PNG or JPEG.
    pub data: Arc<[u8]>,
    /// A bounded PNG used only for display. The original above remains the
    /// authority for printing and export.
    pub preview: Option<Arc<[u8]>>,
    pub preview_pixels: Option<(u32, u32)>,
    pub format: ImageFormat,
    /// Size in pixels.
    pub pixels: (u32, u32),
    /// Size at the picture's own resolution (72 dpi if it gives none).
    pub natural: (Twips, Twips),
    /// Size shown on the page, after scaling and cropping.
    pub size: (Twips, Twips),
    pub crop: Crop,
}

impl PartialEq for Picture {
    fn eq(&self, o: &Picture) -> bool {
        (Arc::ptr_eq(&self.data, &o.data) || self.data == o.data)
            && self.format == o.format
            && self.size == o.size
            && self.crop == o.crop
            && self.natural == o.natural
    }
}

impl Eq for Picture {}

/// What a paragraph's object character stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Object {
    Picture(Picture),
    /// A stable object id whose picture was only reachable from discarded
    /// undo history. Keeping the slot avoids renumbering live paragraphs.
    #[doc(hidden)]
    Vacant,
}

/// Format, pixel size and resolution (dots per inch) of PNG or JPEG data.
pub fn image_info(data: &[u8]) -> Option<(ImageFormat, u32, u32, f32, f32)> {
    let be16 = |i: usize| Some(u16::from_be_bytes([*data.get(i)?, *data.get(i + 1)?]));
    let be32 = |i: usize| {
        Some(u32::from_be_bytes([
            *data.get(i)?,
            *data.get(i + 1)?,
            *data.get(i + 2)?,
            *data.get(i + 3)?,
        ]))
    };
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let (w, h) = (be32(16)?, be32(20)?);
        let mut dpi = (72.0, 72.0);
        // pHYs gives pixels per meter.
        let mut i = 8;
        while i + 8 <= data.len() {
            let len = be32(i)? as usize;
            let kind = data.get(i + 4..i + 8)?;
            if kind == b"pHYs" && len >= 9 && data.get(i + 16) == Some(&1) {
                let (x, y) = (be32(i + 8)? as f32, be32(i + 12)? as f32);
                if x > 0.0 && y > 0.0 {
                    dpi = (x * 0.0254, y * 0.0254);
                }
            }
            if kind == b"IDAT" || kind == b"IEND" {
                break;
            }
            i += 12 + len;
        }
        return (w > 0 && h > 0).then_some((ImageFormat::Png, w, h, dpi.0, dpi.1));
    }
    if data.starts_with(&[0xFF, 0xD8]) {
        let mut dpi = (72.0, 72.0);
        let mut i = 2;
        while i + 4 <= data.len() {
            if data[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = data[i + 1];
            if marker == 0xFF {
                i += 1;
                continue;
            }
            if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
                i += 2;
                continue;
            }
            let len = be16(i + 2)? as usize;
            let body = i + 4;
            if marker == 0xE0 && data.get(body..body + 5) == Some(b"JFIF\0") {
                let units = *data.get(body + 7)?;
                let (x, y) = (be16(body + 8)? as f32, be16(body + 10)? as f32);
                if x > 0.0 && y > 0.0 {
                    match units {
                        1 => dpi = (x, y),
                        2 => dpi = (x * 2.54, y * 2.54),
                        _ => {}
                    }
                }
            }
            let sof = matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
            if sof {
                let (h, w) = (be16(body + 1)? as u32, be16(body + 3)? as u32);
                return (w > 0 && h > 0).then_some((ImageFormat::Jpeg, w, h, dpi.0, dpi.1));
            }
            i += 2 + len;
        }
    }
    None
}

impl Picture {
    /// A picture from a PNG or JPEG file's bytes, at its natural size.
    pub fn from_bytes(data: Vec<u8>) -> Option<Picture> {
        Self::from_shared(data.into())
    }

    /// A picture whose file bytes are already shared with another placement.
    pub fn from_shared(data: Arc<[u8]>) -> Option<Picture> {
        let (format, w, h, dx, dy) = image_info(&data)?;
        let natural = (
            (w as f32 * 1440.0 / dx).round() as Twips,
            (h as f32 * 1440.0 / dy).round() as Twips,
        );
        Some(Picture {
            data,
            preview: None,
            preview_pixels: None,
            format,
            pixels: (w, h),
            natural,
            size: natural,
            crop: Crop::default(),
        })
    }

    /// The natural size of the part that shows, after cropping.
    pub fn shown_natural(&self) -> (f32, f32) {
        let c = &self.crop;
        let w = self.natural.0 as f32 * (CROP_WHOLE - c.left - c.right) as f32 / CROP_WHOLE as f32;
        let h = self.natural.1 as f32 * (CROP_WHOLE - c.top - c.bottom) as f32 / CROP_WHOLE as f32;
        (w.max(1.0), h.max(1.0))
    }

    /// Scale as percentages of the natural size, across and down.
    pub fn scale(&self) -> (f32, f32) {
        let (w, h) = self.shown_natural();
        (
            100.0 * self.size.0 as f32 / w,
            100.0 * self.size.1 as f32 / h,
        )
    }

    /// The same picture scaled to `width` x `height`.
    pub fn scaled_to(&self, width: Twips, height: Twips) -> Picture {
        Picture {
            size: (width.max(20), height.max(20)),
            ..self.clone()
        }
    }

    /// The picture cropped (or padded) to `width` x `height` at its present
    /// scale, keeping its center, as the original's side handles do.
    pub fn cropped_to(&self, width: Twips, height: Twips) -> Picture {
        let (sx, sy) = self.scale();
        let mut p = self.clone();
        let (width, height) = (width.max(20), height.max(20));
        // How much of the natural size each axis now shows.
        let show_w = width as f32 * 100.0 / sx.max(0.01);
        let show_h = height as f32 * 100.0 / sy.max(0.01);
        let to_units =
            |show: f32, natural: Twips| CROP_WHOLE as f32 * (1.0 - show / natural.max(1) as f32);
        let dw = to_units(show_w, p.natural.0) - (p.crop.left + p.crop.right) as f32;
        let dh = to_units(show_h, p.natural.1) - (p.crop.top + p.crop.bottom) as f32;
        p.crop.left += (dw / 2.0).round() as i32;
        p.crop.right += (dw / 2.0).round() as i32;
        p.crop.top += (dh / 2.0).round() as i32;
        p.crop.bottom += (dh / 2.0).round() as i32;
        p.size = (width, height);
        p
    }

    /// The picture cropped (or padded) to `width` x `height` at its present
    /// scale by moving only its right and bottom edges, the ones the side
    /// handles drag. What shows of the rest stays where it was.
    pub fn trimmed_to(&self, width: Twips, height: Twips) -> Picture {
        let (sx, sy) = self.scale();
        let mut p = self.clone();
        let (width, height) = (width.max(20), height.max(20));
        let show_w = width as f32 * 100.0 / sx.max(0.01);
        let show_h = height as f32 * 100.0 / sy.max(0.01);
        let to_units =
            |show: f32, natural: Twips| CROP_WHOLE as f32 * (1.0 - show / natural.max(1) as f32);
        p.crop.right = (to_units(show_w, p.natural.0) - p.crop.left as f32).round() as i32;
        p.crop.bottom = (to_units(show_h, p.natural.1) - p.crop.top as f32).round() as i32;
        p.size = (width, height);
        p
    }

    /// The whole picture's size on the page at its present scale, as if
    /// nothing were cropped.
    pub fn full_size(&self) -> (f32, f32) {
        let c = &self.crop;
        let whole = |shown: Twips, cut: i32| {
            shown as f32 * CROP_WHOLE as f32 / (CROP_WHOLE - cut).max(1) as f32
        };
        (
            whole(self.size.0, c.left + c.right),
            whole(self.size.1, c.top + c.bottom),
        )
    }

    /// How much is cropped from each edge, in twips on the page: left, top,
    /// right, bottom.
    pub fn crop_on_page(&self) -> [f32; 4] {
        let (w, h) = self.full_size();
        let c = &self.crop;
        let on = |cut: i32, full: f32| cut as f32 * full / CROP_WHOLE as f32;
        [on(c.left, w), on(c.top, h), on(c.right, w), on(c.bottom, h)]
    }

    /// The picture with `more` twips on the page cropped from each edge
    /// (left, top, right, bottom), or given back if negative. The scale
    /// stays. An edge that isn't moved, and an axis with no edge moved,
    /// keep their exact values.
    pub fn crop_edges(&self, more: [f32; 4]) -> Picture {
        let (w, h) = self.full_size();
        let mut p = self.clone();
        let step =
            |twips: f32, full: f32| (twips * CROP_WHOLE as f32 / full.max(1.0)).round() as i32;
        p.crop.left += step(more[0], w);
        p.crop.top += step(more[1], h);
        p.crop.right += step(more[2], w);
        p.crop.bottom += step(more[3], h);
        if more[0] != 0.0 || more[2] != 0.0 {
            let shown = CROP_WHOLE - p.crop.left - p.crop.right;
            p.size.0 = ((w * shown as f32 / CROP_WHOLE as f32).round() as Twips).max(20);
        }
        if more[1] != 0.0 || more[3] != 0.0 {
            let shown = CROP_WHOLE - p.crop.top - p.crop.bottom;
            p.size.1 = ((h * shown as f32 / CROP_WHOLE as f32).round() as Twips).max(20);
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2 x 3 PNG header with a 144 dpi pHYs chunk (no pixel data needed
    /// for reading the size).
    fn png_header() -> Vec<u8> {
        let mut d = b"\x89PNG\r\n\x1a\n".to_vec();
        d.extend_from_slice(&13u32.to_be_bytes());
        d.extend_from_slice(b"IHDR");
        d.extend_from_slice(&2u32.to_be_bytes());
        d.extend_from_slice(&3u32.to_be_bytes());
        d.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        d.extend_from_slice(&9u32.to_be_bytes());
        d.extend_from_slice(b"pHYs");
        let ppm = (144.0f32 / 0.0254).round() as u32;
        d.extend_from_slice(&ppm.to_be_bytes());
        d.extend_from_slice(&ppm.to_be_bytes());
        d.extend_from_slice(&[1, 0, 0, 0, 0]);
        d
    }

    #[test]
    fn cropping_edges_keeps_the_scale_and_the_full_size() {
        let mut p = Picture::from_bytes(png_header()).unwrap();
        p = p.scaled_to(7 * 1440, 5 * 1440);
        let half = 720.0;
        let c = p.crop_edges([half, half, half, half]);
        assert_eq!(c.size, (6 * 1440, 4 * 1440));
        let (w, h) = c.full_size();
        assert!((w - 7.0 * 1440.0).abs() < 2.0 && (h - 5.0 * 1440.0).abs() < 2.0);
        assert!(c.crop_on_page().iter().all(|t| (t - half).abs() < 2.0));
        // Giving an edge back; the height keeps its exact value.
        let back = c.crop_edges([-half, 0.0, -half, 0.0]);
        assert_eq!(back.size.0, 7 * 1440);
        assert_eq!(back.size.1, c.size.1);
        assert_eq!((back.crop.left, back.crop.right), (0, 0));
    }

    #[test]
    fn reads_png_and_jpeg_sizes() {
        let (f, w, h, dx, _) = image_info(&png_header()).unwrap();
        assert_eq!((f, w, h), (ImageFormat::Png, 2, 3));
        assert!((dx - 144.0).abs() < 0.5);
        let p = Picture::from_bytes(png_header()).unwrap();
        assert_eq!(p.natural, (20, 30));
        // A JFIF JPEG at 300 dpi, 640 x 480.
        let mut j = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 16];
        j.extend_from_slice(b"JFIF\0");
        j.extend_from_slice(&[1, 1, 1, 0x01, 0x2C, 0x01, 0x2C, 0, 0]);
        j.extend_from_slice(&[0xFF, 0xC0, 0, 17, 8, 0x01, 0xE0, 0x02, 0x80, 3]);
        j.extend_from_slice(&[0; 9]);
        let (f, w, h, dx, _) = image_info(&j).unwrap();
        assert_eq!((f, w, h, dx), (ImageFormat::Jpeg, 640, 480, 300.0));
    }

    #[test]
    fn scaling_and_cropping() {
        let mut p = Picture::from_bytes(png_header()).unwrap();
        p.natural = (1440, 720);
        p.size = p.natural;
        let s = p.scaled_to(2880, 720);
        assert_eq!(s.scale(), (200.0, 100.0));
        // Cropping keeps the scale and the center.
        let c = s.cropped_to(1440, 720);
        assert_eq!(c.crop.left, 25_000);
        assert_eq!(c.crop.right, 25_000);
        assert!((c.scale().0 - 200.0).abs() < 0.1);
        // Wider than the picture: white space on both sides.
        let pad = p.cropped_to(2880, 720);
        assert_eq!(pad.crop.left, -50_000);
    }

    #[test]
    fn trimming_moves_only_the_right_and_bottom_edges() {
        let mut p = Picture::from_bytes(png_header()).unwrap();
        p.natural = (1440, 720);
        p.size = (2880, 720);
        p.crop.left = 10_000;
        // Half as wide: the right edge comes in, the left stays put.
        let t = p.trimmed_to(1440, 360);
        assert_eq!(t.crop.left, 10_000);
        assert_eq!(t.crop.right, 45_000);
        assert_eq!(t.crop.top, 0);
        assert_eq!(t.crop.bottom, 50_000);
        assert!((t.scale().0 - p.scale().0).abs() < 0.1);
        assert!((t.scale().1 - p.scale().1).abs() < 0.1);
        // Dragging back out uncrops the same edge.
        let back = t.trimmed_to(p.size.0, p.size.1);
        assert_eq!(back.crop, p.crop);
    }

    #[test]
    fn preview_is_shared_but_not_part_of_picture_identity() {
        let mut p = Picture::from_bytes(png_header()).unwrap();
        let same = p.clone();
        p.preview = Some(Arc::from(&b"preview"[..]));
        p.preview_pixels = Some((1, 1));
        assert_eq!(p, same);
        let scaled = p.scaled_to(40, 50);
        assert!(Arc::ptr_eq(
            scaled.preview.as_ref().unwrap(),
            p.preview.as_ref().unwrap()
        ));
    }

    #[test]
    fn save_copy_drops_undo_only_pictures_and_remaps_ids() {
        use crate::{Document, Position, StoryId};
        let mut doc = Document::new();
        let chp = doc.main.paras[0].mark_chp;
        let old = Picture::from_bytes(png_header()).unwrap();
        let at = Position::new(0, 0);
        doc.insert_picture(StoryId::Main, at, old.clone(), chp);
        let replacement = old.scaled_to(99, 99);
        doc.set_picture(StoryId::Main, at, replacement.clone());
        assert_eq!(doc.objects.len(), 2);
        let saved = doc.compact_for_save();
        assert_eq!(saved.objects.len(), 1);
        assert_eq!(saved.main.paras[0].runs[0].chp.object, 1);
        assert_eq!(saved.picture(1), Some(&replacement));
    }

    #[test]
    fn pictures_travel_with_copy_and_paste() {
        use crate::{Document, Position, StoryId};
        let mut a = Document::new();
        let chp = a.main.paras[0].mark_chp;
        let pic = Picture::from_bytes(png_header()).unwrap();
        a.insert_text(StoryId::Main, Position::new(0, 0), "ab", chp);
        a.insert_picture(StoryId::Main, Position::new(0, 1), pic.clone(), chp);
        let frag = a.copy(StoryId::Main, Position::new(0, 0), Position::new(0, 5));
        assert_eq!(frag.objects.len(), 1);
        let mut b = Document::new();
        // b already has a different picture, so the pasted one gets id 2.
        let mut other = pic.clone();
        other.size = (99, 99);
        b.insert_picture(StoryId::Main, Position::new(0, 0), other, chp);
        b.insert_fragment(StoryId::Main, Position::new(0, 3), &frag);
        let id = b.main.paras[0].chp_of_char(4).object;
        assert_eq!(id, 2);
        assert_eq!(b.picture(id), Some(&pic));
    }
}
