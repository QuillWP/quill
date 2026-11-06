//! Style sheets. In the original, a style is a paragraph style that carries both
//! paragraph and character formatting.

use crate::props::{CharProps, ParaProps, StyleId, TabAlign, TabLeader, TabStop, Twips, Underline};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Style {
    pub name: String,
    pub based_on: Option<StyleId>,
    /// Style for the paragraph Return makes at the end of this one.
    pub next: StyleId,
    /// Complete formatting, with `pap.style` set to this style's id.
    pub pap: ParaProps,
    pub chp: CharProps,
    /// One of the original's automatic styles (Normal, heading 1, ...).
    pub builtin: bool,
    /// A named character style this style's text takes, numbered from one;
    /// zero for none. While one is linked, `chp` is it laid over `own_chp`.
    pub character: u16,
    /// The style's own character formatting, kept while a character style
    /// is linked so that unlinking brings it back.
    pub own_chp: CharProps,
}

impl Style {
    /// The style's own character formatting: `chp` itself unless a
    /// character style is linked.
    pub fn own(&self) -> CharProps {
        if self.character == 0 {
            self.chp
        } else {
            self.own_chp
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleSheet {
    pub characters: Vec<crate::character_styles::CharacterStyle>,
    pub styles: Vec<Style>,
}

/// Normal is always style 0.
pub const NORMAL: StyleId = 0;

const INCH: Twips = 1440;

impl StyleSheet {
    /// The original's automatic styles, built on a Normal that uses `font` at
    /// `size` half-points. `font_id` interns font names in the document.
    pub fn standard(font: u16, size: u16, font_id: &mut impl FnMut(&str) -> u16) -> StyleSheet {
        let normal_chp = CharProps {
            font,
            size,
            ..Default::default()
        };
        let mut sheet = StyleSheet {
            styles: Vec::new(),
            characters: Vec::new(),
        };
        let add = |sheet: &mut StyleSheet,
                   name: &str,
                   based_on: Option<StyleId>,
                   f: &mut dyn FnMut(&mut ParaProps, &mut CharProps)| {
            let id = sheet.styles.len() as StyleId;
            let (mut pap, mut chp) = match based_on {
                Some(b) => (
                    sheet.styles[b as usize].pap.clone(),
                    sheet.styles[b as usize].chp,
                ),
                None => (ParaProps::default(), normal_chp),
            };
            pap.style = id;
            f(&mut pap, &mut chp);
            sheet.styles.push(Style {
                name: name.to_string(),
                based_on,
                next: id,
                pap,
                chp,
                builtin: true,
                character: 0,
                own_chp: chp,
            });
            id
        };

        add(&mut sheet, "Normal", None, &mut |_, _| {});
        let helvetica = font_id("Helvetica");
        // Heading defaults follow the original's: big headings in Helvetica, the
        // rest Normal with bold, underline or italic and a growing indent.
        for level in 1..=9u16 {
            add(
                &mut sheet,
                &format!("heading {level}"),
                Some(NORMAL),
                &mut |p, c| {
                    match level {
                        1 => {
                            c.font = helvetica;
                            c.bold = true;
                            c.underline = Underline::Single;
                            p.before = 240;
                        }
                        2 => {
                            c.font = helvetica;
                            c.bold = true;
                            p.before = 120;
                        }
                        3 => {
                            c.bold = true;
                            p.left = INCH / 4;
                        }
                        4 => {
                            c.underline = Underline::Single;
                            p.left = INCH / 4;
                        }
                        5 => {
                            c.bold = true;
                            p.left = INCH / 2;
                        }
                        6 => {
                            c.underline = Underline::Single;
                            p.left = INCH / 2;
                        }
                        _ => {
                            c.italic = true;
                            p.left = INCH / 2;
                        }
                    }
                    p.keep_with_next = true;
                },
            );
        }
        let center_right = |p: &mut ParaProps| {
            p.set_tab(TabStop {
                pos: 3 * INCH,
                align: TabAlign::Center,
                leader: TabLeader::None,
            });
            p.set_tab(TabStop {
                pos: 6 * INCH,
                align: TabAlign::Right,
                leader: TabLeader::None,
            });
        };
        add(&mut sheet, "header", Some(NORMAL), &mut |p, _| {
            center_right(p)
        });
        add(&mut sheet, "footer", Some(NORMAL), &mut |p, _| {
            center_right(p)
        });
        add(&mut sheet, "footnote text", Some(NORMAL), &mut |_, c| {
            c.size = 20
        });
        add(
            &mut sheet,
            "footnote reference",
            Some(NORMAL),
            &mut |_, c| {
                c.size = 18;
                c.position = 60;
            },
        );
        add(&mut sheet, "page number", Some(NORMAL), &mut |_, _| {});
        add(&mut sheet, "line number", Some(NORMAL), &mut |_, _| {});
        for level in 1..=7 {
            add(
                &mut sheet,
                &format!("index {level}"),
                Some(NORMAL),
                &mut |p, _| {
                    p.left = (level - 1) * INCH / 4;
                },
            );
        }
        for level in 1..=9 {
            add(
                &mut sheet,
                &format!("toc {level}"),
                Some(NORMAL),
                &mut |p, _| {
                    p.left = (level - 1) * INCH / 4;
                    p.right = INCH / 2;
                    p.set_tab(TabStop {
                        pos: 6 * INCH,
                        align: TabAlign::Right,
                        leader: TabLeader::Dots,
                    });
                },
            );
        }
        let monaco = font_id("Monaco");
        add(&mut sheet, "PostScript", Some(NORMAL), &mut |_, c| {
            c.font = monaco;
            c.bold = true;
            c.hidden = true;
        });
        // Headings are followed by Normal text.
        for s in &mut sheet.styles {
            if s.name.starts_with("heading ") {
                s.next = NORMAL;
            }
        }
        sheet
    }

    pub fn get(&self, id: StyleId) -> &Style {
        self.styles.get(id as usize).unwrap_or(&self.styles[0])
    }

    /// The character formatting a paragraph style gives its text: `own`, or
    /// character style `character` laid over it.
    pub fn linked_chp(&self, own: CharProps, character: u16) -> CharProps {
        match self.character(character) {
            Some(c) => c.format.resolved(own),
            None => own,
        }
    }

    pub fn find(&self, name: &str) -> Option<StyleId> {
        self.styles
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
            .map(|i| i as StyleId)
    }

    /// Outline level of a heading style (1-9), if it is one.
    pub fn heading_level(&self, id: StyleId) -> Option<u8> {
        self.numbered_level(id, "heading")
    }

    /// Level of a table-of-contents style (1-9), if it is one.
    pub fn toc_level(&self, id: StyleId) -> Option<u8> {
        self.numbered_level(id, "toc")
    }

    fn numbered_level(&self, id: StyleId, kind: &str) -> Option<u8> {
        let (prefix, number) = self.get(id).name.rsplit_once(' ')?;
        if !prefix.eq_ignore_ascii_case(kind) {
            return None;
        }
        number.parse().ok().filter(|n| (1..=9).contains(n))
    }

    /// Add a new user style and return its id.
    pub fn add(
        &mut self,
        name: &str,
        based_on: StyleId,
        pap: ParaProps,
        chp: CharProps,
    ) -> StyleId {
        let id = self.styles.len() as StyleId;
        let mut pap = pap;
        pap.style = id;
        self.styles.push(Style {
            name: name.to_string(),
            based_on: Some(based_on),
            next: id,
            pap,
            chp,
            builtin: false,
            character: 0,
            own_chp: chp,
        });
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> StyleSheet {
        let mut names = vec!["New York".to_string()];
        StyleSheet::standard(0, 24, &mut |n| {
            if let Some(i) = names.iter().position(|x| x == n) {
                return i as u16;
            }
            names.push(n.to_string());
            (names.len() - 1) as u16
        })
    }

    #[test]
    fn has_standard_automatic_styles() {
        let s = sheet();
        for name in [
            "Normal",
            "heading 1",
            "heading 9",
            "header",
            "footer",
            "footnote text",
            "footnote reference",
            "index 7",
            "toc 9",
            "line number",
            "page number",
            "PostScript",
        ] {
            assert!(s.find(name).is_some(), "missing {name}");
        }
        assert_eq!(s.find("Normal"), Some(NORMAL));
        assert_eq!(s.styles.len(), 1 + 9 + 2 + 2 + 2 + 7 + 9 + 1);
    }

    #[test]
    fn styles_know_their_own_id() {
        let s = sheet();
        for (i, style) in s.styles.iter().enumerate() {
            assert_eq!(style.pap.style as usize, i);
        }
    }

    #[test]
    fn header_has_center_and_right_tabs() {
        let s = sheet();
        let h = s.get(s.find("header").unwrap());
        let tabs: Vec<_> = h.pap.tabs.iter().map(|t| (t.pos, t.align)).collect();
        assert_eq!(tabs, [(4320, TabAlign::Center), (8640, TabAlign::Right)]);
    }

    #[test]
    fn numbered_styles_accept_case_variations_and_only_supported_levels() {
        let mut s = sheet();
        for level in 1..=9 {
            let heading = s.find(&format!("heading {level}")).unwrap();
            let toc = s.find(&format!("toc {level}")).unwrap();
            s.styles[heading as usize].name = format!("Heading {level}");
            s.styles[toc as usize].name = format!("TOC {level}");
            assert_eq!(s.heading_level(heading), Some(level));
            assert_eq!(s.toc_level(toc), Some(level));
        }
        let toc = s.find("toc 6").unwrap();
        for name in ["TOC 0", "TOC 10", "TOC notes"] {
            s.styles[toc as usize].name = name.into();
            assert_eq!(s.toc_level(toc), None);
        }
    }

    #[test]
    fn heading_levels() {
        let s = sheet();
        assert_eq!(s.heading_level(s.find("heading 3").unwrap()), Some(3));
        assert_eq!(s.heading_level(NORMAL), None);
        assert_eq!(s.get(s.find("heading 1").unwrap()).next, NORMAL);
    }
}
