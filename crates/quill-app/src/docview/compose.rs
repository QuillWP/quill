//! Text an input method is still composing: Chinese, Japanese or Korean
//! before it's chosen, or an accent typed with a dead key. It shows at the
//! insertion point, underlined, until the input method commits it as typing.

use super::*;
use quill_layout::{FaceKey, GlyphId};

/// The composition, shaped in the font at the insertion point.
pub(crate) struct Composing {
    glyphs: Vec<(FaceKey, GlyphId, f32)>,
    width: f32,
    size: f32,
    color: u32,
}

impl DocView {
    /// Show `text` as what the input method is composing; empty ends it.
    pub fn set_composing(&mut self, text: &str, fonts: &mut Fonts) {
        if text.is_empty() {
            self.composing = None;
            return;
        }
        let at = self.sel.focus;
        let chp = self.typing_chp.unwrap_or_else(|| {
            let story = &self.doc.main.paras;
            story
                .get(at.para)
                .map_or_else(CharProps::default, |p| p.chp_at(at.offset))
        });
        let size = chp.size as f32 / 2.0;
        let face = fonts.resolve(self.doc.font_name(chp.font), chp.bold, chp.italic);
        let mut glyphs = Vec::new();
        let mut x = 0.0;
        for ch in text.chars() {
            let (f, id) = fonts.glyph(face, ch);
            glyphs.push((f, id, x));
            x += fonts.advance(f, id, size);
        }
        self.composing = Some(Composing {
            glyphs,
            width: x,
            size,
            color: chp.color.0,
        });
        self.caret_since = Instant::now();
    }

    pub fn end_composing(&mut self) {
        self.composing = None;
    }

    pub fn is_composing(&self) -> bool {
        self.composing.is_some()
    }

    /// The insertion point in window coordinates (x, top, height), where the
    /// input method puts its list of choices.
    pub fn caret_in_window(&self) -> (f32, f32, f32) {
        let (x, top, h) = self.caret_geometry(self.sel.focus, self.upstream);
        let (wx, wy) = self.to_window(x, top);
        (wx, wy, h)
    }

    /// Draw the composition over the text at the insertion point, on white
    /// so the text it covers doesn't show through.
    pub(super) fn draw_composing(
        &self,
        c: &mut dyn Surface,
        fonts: &Fonts,
        cache: &mut GlyphCache,
    ) {
        let Some(comp) = &self.composing else {
            return;
        };
        let pos = self.sel.focus;
        let l = self.layout(pos.para);
        let (li, x) = l.caret(pos.offset, self.upstream);
        let line = &l.lines[li];
        let (wx, wy) = self.to_window(self.xs[pos.para] + x, self.tops[pos.para] + line.top);
        c.fill(
            Rect::new(wx, wy, comp.width + 1.0, line.height),
            color::WHITE,
        );
        let baseline = wy + line.baseline;
        for &(face, id, dx) in &comp.glyphs {
            c.glyph(
                fonts,
                cache,
                face,
                id,
                comp.size,
                wx + dx,
                baseline,
                comp.color,
            );
        }
        c.hline(wx, wx + comp.width, baseline + 2.0, comp.color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composing_shows_at_the_caret_without_changing_the_document() {
        let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
        if fonts.family_names().is_empty() {
            return;
        }
        let mut view = DocView::new(Document::new(), "Compose".into());
        view.type_text("ab");
        view.update_layout(&mut fonts);
        view.area = Rect::new(0.0, 0.0, 400.0, 300.0);
        view.set_composing("\u{304B}", &mut fonts);
        assert!(view.is_composing());
        assert_eq!(view.doc.main.paras[0].text, "ab");

        // Drawn over the text at the insertion point.
        let mut plain = vec![0u32; 400 * 300];
        let mut composed = plain.clone();
        let cache = &mut GlyphCache::default();
        view.end_composing();
        view.draw(
            &mut Canvas::new(&mut plain, 400, 300, 1.0),
            &fonts,
            cache,
            true,
        );
        view.set_composing("\u{304B}", &mut fonts);
        view.draw(
            &mut Canvas::new(&mut composed, 400, 300, 1.0),
            &fonts,
            cache,
            true,
        );
        assert_ne!(plain, composed);

        // Committed, it is typed in; an empty composition ends it.
        view.type_text("\u{304B}");
        view.end_composing();
        assert_eq!(view.doc.main.paras[0].text, "ab\u{304B}");
        view.set_composing("x", &mut fonts);
        view.set_composing("", &mut fonts);
        assert!(!view.is_composing());
    }
}
