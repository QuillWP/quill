//! Fonts, glyph rasterising and text layout for Quill.

pub mod fonts;
pub mod layout;
pub mod paginate;
pub mod raster;

pub use ab_glyph;
pub use ab_glyph::GlyphId;
pub use fonts::{Catalog, Curve, FaceId, FaceKey, FontFamilyInfo, FontVariant, Fonts};
pub use layout::{
    BorderBox, Clear, Joins, LayoutOptions, ParaLayout, border_width, joins, layout_paragraph,
    layout_paragraph_joined,
};
pub use raster::{GlyphCache, ITALIC_SHEAR, Mask};

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::props::{Align, CharProps, ParaProps, TabAlign, TabStop};
    use quill_core::{Paragraph, Twips};

    fn fonts() -> Option<Fonts> {
        let f = Fonts::new(Catalog::scan_system());
        (!f.family_names().is_empty()).then_some(f)
    }

    fn names(_: u16) -> String {
        "Times".to_string()
    }

    fn para(text: &str, pap: ParaProps) -> Paragraph {
        Paragraph::with_text(text, CharProps::default(), pap)
    }

    fn lay(p: &Paragraph, width: f32, fonts: &mut Fonts) -> ParaLayout {
        let opts = LayoutOptions {
            width,
            ..Default::default()
        };
        layout_paragraph(p, &names, fonts, &opts)
    }

    #[test]
    fn empty_paragraph_has_one_line_of_height() {
        let Some(mut f) = fonts() else { return };
        let l = lay(&para("", ParaProps::default()), 400.0, &mut f);
        assert_eq!(l.lines.len(), 1);
        assert!(l.height > 10.0 && l.height < 20.0, "{}", l.height);
    }

    #[test]
    fn long_text_wraps_at_spaces() {
        let Some(mut f) = fonts() else { return };
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(6);
        let l = lay(&para(&text, ParaProps::default()), 200.0, &mut f);
        assert!(l.lines.len() > 3);
        for line in &l.lines {
            let g = &l.glyphs[line.first..line.last];
            let last_text = g
                .iter()
                .rev()
                .find(|g| g.kind == layout::Kind::Text)
                .unwrap();
            assert!(last_text.x + last_text.advance <= 200.5, "line overflows");
            // Lines break after a space, so they start with a letter.
            assert_ne!(text.as_bytes()[line.start], b' ');
        }
        // Lines cover the whole text without gaps.
        assert_eq!(l.lines[0].start, 0);
        for w in l.lines.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        assert_eq!(l.lines.last().unwrap().end, text.len());
    }

    #[test]
    fn centered_and_right_aligned() {
        let Some(mut f) = fonts() else { return };
        let mut pap = ParaProps {
            align: Align::Center,
            ..Default::default()
        };
        let c = lay(&para("Hello", pap.clone()), 400.0, &mut f);
        pap.align = Align::Right;
        let r = lay(&para("Hello", pap), 400.0, &mut f);
        let cl = c.lines[0].left;
        let rl = r.lines[0].left;
        assert!(cl > 150.0 && cl < 200.0, "{cl}");
        assert!((r.lines[0].end_x - 400.0).abs() < 0.5);
        assert!(rl > cl);
    }

    #[test]
    fn justified_lines_fill_the_width() {
        let Some(mut f) = fonts() else { return };
        let pap = ParaProps {
            align: Align::Justify,
            ..Default::default()
        };
        let text = "aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn ooo ppp";
        let l = lay(&para(text, pap), 150.0, &mut f);
        let first = &l.lines[0];
        let g = &l.glyphs[first.first..first.last];
        let last_text = g
            .iter()
            .rev()
            .find(|g| g.kind == layout::Kind::Text)
            .unwrap();
        assert!((last_text.x + last_text.advance - 150.0).abs() < 0.5);
    }

    #[test]
    fn default_and_custom_tabs() {
        let Some(mut f) = fonts() else { return };
        let l = lay(&para("a\tb", ParaProps::default()), 400.0, &mut f);
        let b = l.glyphs[2];
        assert!(
            (b.x - 36.0).abs() < 0.01,
            "default tab at 0.5 in, got {}",
            b.x
        );

        let mut pap = ParaProps::default();
        let inch: Twips = 1440;
        pap.set_tab(TabStop {
            pos: 2 * inch,
            align: TabAlign::Right,
            ..Default::default()
        });
        let l = lay(&para("a\tright", pap), 400.0, &mut f);
        let last = l.glyphs.last().unwrap();
        assert!(
            (last.x + last.advance - 144.0).abs() < 0.01,
            "right tab ends at 2 in"
        );
    }

    #[test]
    fn decimal_tab_aligns_on_point() {
        let Some(mut f) = fonts() else { return };
        let mut pap = ParaProps::default();
        pap.set_tab(TabStop {
            pos: 1440,
            align: TabAlign::Decimal,
            ..Default::default()
        });
        let l = lay(&para("\t12.50", pap), 400.0, &mut f);
        let point = l.glyphs.iter().find(|g| g.offset == 3).unwrap();
        assert!(
            (point.x - 72.0).abs() < 0.01,
            "point at 1 in, got {}",
            point.x
        );
    }

    #[test]
    fn caret_and_hit_testing_agree() {
        let Some(mut f) = fonts() else { return };
        let text = "Hello world, this wraps around a few times for sure.";
        let l = lay(&para(text, ParaProps::default()), 120.0, &mut f);
        for (offset, _) in text.char_indices() {
            let (li, x) = l.caret(offset, false);
            let (hit, _) = l.hit(li, x + 0.1);
            assert_eq!(hit, offset, "offset {offset}");
        }
    }

    #[test]
    fn line_breaks_and_indents() {
        let Some(mut f) = fonts() else { return };
        let pap = ParaProps {
            left: 720,
            first: -360,
            ..Default::default()
        };
        let l = lay(&para("one\u{000B}two", pap), 400.0, &mut f);
        assert_eq!(l.lines.len(), 2);
        assert!((l.lines[0].left - 18.0).abs() < 0.01);
        assert!((l.lines[1].left - 36.0).abs() < 0.01);
    }

    #[test]
    fn right_tab_at_the_margin_does_not_wrap() {
        use quill_core::props::{TabAlign, TabLeader, TabStop};
        let Some(mut f) = fonts() else { return };
        let pap = ParaProps {
            tabs: vec![TabStop {
                pos: 400 * 20,
                align: TabAlign::Right,
                leader: TabLeader::None,
            }],
            ..Default::default()
        };
        let l = lay(&para("Left\tRight", pap), 400.0, &mut f);
        assert_eq!(l.lines.len(), 1);
        let last = l
            .glyphs
            .iter()
            .rfind(|g| g.kind == layout::Kind::Text)
            .unwrap();
        assert!((last.x + last.advance - 400.0).abs() < 0.5, "{}", last.x);
    }

    #[test]
    fn lines_flow_round_a_clear_area() {
        let Some(mut f) = fonts() else { return };
        let text = "word ".repeat(60);
        let p = para(&text, ParaProps::default());
        let names = |_: u16| "Helvetica".to_string();
        // A frame 100 points wide at the left, 30 points tall.
        let opts = LayoutOptions {
            width: 300.0,
            wrap: vec![Clear {
                top: 0.0,
                bottom: 30.0,
                left: 0.0,
                right: 100.0,
            }],
            ..Default::default()
        };
        let l = layout_paragraph(&p, &names, &mut f, &opts);
        assert!(l.lines[0].left >= 100.0, "{}", l.lines[0].left);
        let below = l.lines.iter().find(|ln| ln.top >= 30.0).unwrap();
        assert!(below.left < 1.0);
        // A frame leaving under an inch beside it pushes the lines below it.
        let opts = LayoutOptions {
            wrap: vec![Clear {
                top: 0.0,
                bottom: 30.0,
                left: 0.0,
                right: 250.0,
            }],
            ..opts
        };
        let l = layout_paragraph(&p, &names, &mut f, &opts);
        assert!(l.lines[0].top >= 30.0);
    }
}
