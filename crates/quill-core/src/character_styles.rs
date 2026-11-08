//! Named character formatting layered over a paragraph's character defaults.

use crate::para::Paragraph;
use crate::{CharProps, Document, Position, StoryId};

/// Formatting fields, excluding object, hyperlink, revision and comment identity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CharacterFormat {
    pub mask: u16,
    pub props: CharProps,
}

macro_rules! fields {
    ($apply:ident) => {
        $apply!(font, 0);
        $apply!(size, 1);
        $apply!(bold, 2);
        $apply!(italic, 3);
        $apply!(underline, 4);
        $apply!(strike, 5);
        $apply!(outline, 6);
        $apply!(shadow, 7);
        $apply!(small_caps, 8);
        $apply!(all_caps, 9);
        $apply!(hidden, 10);
        $apply!(color, 11);
        $apply!(position, 12);
        $apply!(spacing, 13);
        $apply!(script, 14);
    };
}

impl CharacterFormat {
    pub const ALL: u16 = (1 << 15) - 1;

    pub fn difference(a: CharProps, b: CharProps) -> u16 {
        let mut mask = 0;
        macro_rules! compare {
            ($field:ident, $bit:expr) => {
                if a.$field != b.$field {
                    mask |= 1 << $bit;
                }
            };
        }
        fields!(compare);
        mask
    }

    pub fn copy_fields(target: &mut CharProps, source: CharProps, mask: u16) {
        macro_rules! copy {
            ($field:ident, $bit:expr) => {
                if mask & (1 << $bit) != 0 {
                    target.$field = source.$field;
                }
            };
        }
        fields!(copy);
    }

    pub fn new(props: CharProps, mask: u16) -> Self {
        let mut clean = CharProps::default();
        Self::copy_fields(&mut clean, props, mask);
        clean.keep_in_bounds();
        Self {
            props: clean,
            mask: mask & Self::ALL,
        }
    }

    pub fn from_selection(mut props: CharProps, paragraph: CharProps) -> Self {
        let mask = Self::difference(props, paragraph);
        macro_rules! toggle { ($($field:ident),*) => { $(props.$field ^= paragraph.$field;)* }; }
        toggle!(
            bold, italic, strike, outline, shadow, small_caps, all_caps, hidden
        );
        Self::new(props, mask)
    }

    /// Boolean style properties toggle their paragraph defaults, matching
    /// DOCX and RTF character-style inheritance. Direct formatting is absolute.
    pub fn resolved(self, paragraph: CharProps) -> CharProps {
        let mut out = paragraph;
        Self::copy_fields(&mut out, self.props, self.mask);
        macro_rules! toggle { ($($field:ident, $bit:expr);*) => { $(if self.mask & (1 << $bit) != 0 { out.$field ^= paragraph.$field; })* }; }
        toggle!(bold, 2; italic, 3; strike, 5; outline, 6; shadow, 7; small_caps, 8; all_caps, 9; hidden, 10);
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacterStyle {
    pub name: String,
    pub format: CharacterFormat,
}

impl crate::StyleSheet {
    /// Zero means no named character style; definitions are numbered from one.
    pub fn character(&self, id: u16) -> Option<&CharacterStyle> {
        id.checked_sub(1)
            .and_then(|i| self.characters.get(i as usize))
    }

    pub fn find_character(&self, name: &str) -> Option<u16> {
        self.characters
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
            .map(|i| (i + 1) as u16)
    }

    pub fn add_character(&mut self, name: &str, format: CharacterFormat) -> Option<u16> {
        let name = name.trim();
        if name.is_empty()
            || name.chars().count() > 255
            || self.characters.len() >= u16::MAX as usize
            || self.find_character(name).is_some()
        {
            return None;
        }
        self.characters.push(CharacterStyle {
            name: name.into(),
            format: CharacterFormat::new(format.props, format.mask),
        });
        Some(self.characters.len() as u16)
    }

    /// Change the named layer while retaining unrelated formatting and metadata.
    pub fn with_character_style(
        &self,
        mut c: CharProps,
        paragraph: CharProps,
        id: u16,
    ) -> CharProps {
        if id != 0 && self.character(id).is_none() {
            return c;
        }
        if c.character_style == 0 {
            c.direct_format = CharacterFormat::difference(c, paragraph);
        }
        if let Some(old) = self.character(c.character_style) {
            let mask = old.format.mask & !c.direct_format;
            CharacterFormat::copy_fields(&mut c, paragraph, mask);
        }
        if let Some(new) = self.character(id) {
            CharacterFormat::copy_fields(&mut c, new.format.resolved(paragraph), new.format.mask);
            c.direct_format &= !new.format.mask;
        } else {
            c.direct_format = 0;
        }
        c.character_style = id;
        c
    }

    /// Text can land under another paragraph, by a join or a paste, and it
    /// keeps its look. What no longer follows from its style and paragraph
    /// counts as an override, so a redefinition neither loses nor reverts it.
    pub fn keep_overrides(&self, p: &mut Paragraph) {
        if p.mark_chp.character_style == 0 && p.runs.iter().all(|r| r.chp.character_style == 0) {
            return;
        }
        let paragraph = self.get(p.pap.style).chp;
        let mut keep = |c: &mut CharProps| {
            if let Some(style) = self.character(c.character_style) {
                let mut expected = paragraph;
                CharacterFormat::copy_fields(
                    &mut expected,
                    style.format.resolved(paragraph),
                    style.format.mask,
                );
                c.direct_format |= CharacterFormat::difference(*c, expected);
            }
        };
        let len = p.len();
        p.format_range(0, len, &mut keep);
        keep(&mut p.mark_chp);
    }

    pub fn keep_character_format(
        &self,
        before: CharProps,
        after: &mut CharProps,
        paragraph: CharProps,
    ) {
        if let Some(style) = self.character(before.character_style) {
            CharacterFormat::copy_fields(
                after,
                style.format.resolved(paragraph),
                style.format.mask & !before.direct_format,
            );
            CharacterFormat::copy_fields(after, before, before.direct_format);
        }
    }
}

impl Document {
    /// Match imported definitions without replacing a different existing style.
    /// Pane synchronisation, like font import, has no separate undo transaction.
    pub fn import_character_style(
        &mut self,
        source: &CharacterStyle,
        fonts: &[String],
        undoable: bool,
    ) -> u16 {
        let mut format = source.format;
        if format.mask & 1 != 0 {
            format.props.font = fonts
                .get(format.props.font as usize)
                .map_or(0, |name| self.font_id(name));
        }
        let mut name = source.name.clone();
        let mut suffix = 0;
        loop {
            if let Some(id) = self.styles.find_character(&name) {
                if self.styles.character(id).unwrap().format == format {
                    return id;
                }
                suffix += 1;
                let base: String = source.name.chars().take(220).collect();
                name = format!("{base} (Pasted {suffix})");
            } else {
                return if undoable {
                    self.add_character_style(&name, format)
                } else {
                    self.styles.add_character(&name, format)
                }
                .unwrap_or(0);
            }
        }
    }

    pub fn import_styled_story(
        &mut self,
        id: StoryId,
        source: &crate::Story,
        styles: &crate::StyleSheet,
        fonts: &[String],
        objects: &[crate::Object],
    ) {
        let mut map = std::collections::BTreeMap::new();
        for p in &source.paras {
            for c in std::iter::once(&p.mark_chp).chain(p.runs.iter().map(|r| &r.chp)) {
                if c.character_style != 0 && !map.contains_key(&c.character_style) {
                    let mapped = styles
                        .character(c.character_style)
                        .map_or(0, |s| self.import_character_style(s, fonts, false));
                    map.insert(c.character_style, mapped);
                }
            }
        }
        let mut story = source.clone();
        for p in &mut story.paras {
            let remap = |c: &mut CharProps| {
                c.character_style = map.get(&c.character_style).copied().unwrap_or(0);
                if c.character_style == 0 {
                    c.direct_format = 0;
                }
            };
            p.format_range(0, p.len(), &mut |c| remap(c));
            remap(&mut p.mark_chp);
        }
        self.import_story(id, &story, fonts, objects);
    }

    pub fn add_character_style(&mut self, name: &str, format: CharacterFormat) -> Option<u16> {
        let name = name.trim();
        if name.is_empty()
            || name.chars().count() > 255
            || self.styles.characters.len() >= u16::MAX as usize
            || self.styles.find_character(name).is_some()
        {
            return None;
        }
        self.record_sheet();
        self.styles.add_character(name, format)
    }

    pub fn rename_character_style(&mut self, id: u16, name: &str) -> bool {
        let name = name.trim();
        let Some(style) = self.styles.character(id) else {
            return false;
        };
        if name.is_empty()
            || name.chars().count() > crate::limits::MAX_NAME
            || style.name == name
            || self
                .styles
                .find_character(name)
                .is_some_and(|other| other != id)
        {
            return false;
        }
        self.record_sheet();
        self.styles.characters[id as usize - 1].name = name.into();
        true
    }

    /// Apply only the fields the named style defines. Marks are included only
    /// when the selection passes them, just like ordinary character formatting.
    pub fn apply_character_style(
        &mut self,
        story: StoryId,
        start: Position,
        end: Position,
        id: u16,
    ) {
        if id != 0 && self.styles.character(id).is_none() {
            return;
        }
        let (start, end) = (start.min(end), start.max(end));
        if start == end {
            return;
        }
        let by = self.tracking();
        for i in start.para..=end.para {
            let mut p = self.story(story).paras[i].clone();
            let base = self.styles.get(p.pap.style).chp;
            let a = if i == start.para { start.offset } else { 0 };
            let b = if i == end.para { end.offset } else { p.len() };
            p.format_range(a, b, &mut |c| {
                let before = *c;
                *c = self.styles.with_character_style(*c, base, id);
                if let Some(by) = &by {
                    Document::track_format(before, c, by);
                }
            });
            if i < end.para {
                let before = p.mark_chp;
                p.mark_chp = self.styles.with_character_style(p.mark_chp, base, id);
                if let Some(by) = &by {
                    Document::track_format(before, &mut p.mark_chp, by);
                }
            }
            if p != self.story(story).paras[i] {
                self.splice(story, i, 1, vec![p]);
            }
        }
    }

    /// Redefinition updates every story while retaining direct overrides.
    pub fn redefine_character_style(&mut self, id: u16, format: CharacterFormat) -> bool {
        let Some(old) = self.styles.character(id).cloned() else {
            return false;
        };
        let format = CharacterFormat::new(format.props, format.mask);
        if old.format == format {
            return false;
        }
        self.record_sheet();
        self.styles.characters[id as usize - 1].format = format;
        self.update_character_uses(id, old.format, false);
        self.relink_paragraph_styles(id);
        true
    }

    /// Detach uses without changing their visible formatting; subsequent IDs
    /// move down with the definition, including uses in section headers.
    pub fn delete_character_style(&mut self, id: u16) -> bool {
        let Some(old) = self.styles.character(id).cloned() else {
            return false;
        };
        self.record_sheet();
        self.styles.characters.remove(id as usize - 1);
        self.update_character_uses(id, old.format, true);
        // Paragraph styles that took it keep their look, now as their own.
        for s in &mut self.styles.styles {
            if s.character == id {
                s.character = 0;
                s.own_chp = s.chp;
            } else if s.character > id {
                s.character -= 1;
            }
        }
        true
    }

    fn update_character_uses(&mut self, id: u16, old: CharacterFormat, delete: bool) {
        let header = self.header_section;
        for (section, story) in self.every_story() {
            self.header_section = section;
            for i in 0..self.story(story).paras.len() {
                let mut p = self.story(story).paras[i].clone();
                let base = self.styles.get(p.pap.style).chp;
                let update = |c: &mut CharProps| {
                    if c.character_style == id {
                        if delete {
                            c.character_style = 0;
                            c.direct_format = 0;
                        } else {
                            let new = self.styles.character(id).unwrap().format;
                            CharacterFormat::copy_fields(
                                c,
                                base,
                                old.mask & !new.mask & !c.direct_format,
                            );
                            CharacterFormat::copy_fields(
                                c,
                                new.resolved(base),
                                new.mask & !c.direct_format,
                            );
                        }
                    } else if delete && c.character_style > id {
                        c.character_style -= 1;
                    }
                    if delete
                        && let Some(revision) = crate::revision::get(c.rev)
                        && let Some((_, before)) = &revision.formatted
                    {
                        let mut before = *before;
                        if before.character_style == id {
                            before.character_style = 0;
                            before.direct_format = 0;
                        } else if before.character_style > id {
                            before.character_style -= 1;
                        }
                        c.rev = crate::revision::update(c.rev, |r| {
                            if let Some((_, old)) = &mut r.formatted {
                                *old = before;
                            }
                        });
                    }
                };
                p.format_range(0, p.len(), &mut |c| update(c));
                update(&mut p.mark_chp);
                if p != self.story(story).paras[i] {
                    self.splice(story, i, 1, vec![p]);
                }
            }
        }
        self.header_section = header;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rgb, Selection};
    const MAIN: StoryId = StoryId::Main;

    /// A document with a heading paragraph and a Callout style (Bold, 18 pt).
    fn linked_fixture() -> (Document, crate::props::StyleId, u16) {
        let mut d = crate::text::from_plain_text("Title\nBody");
        let heading = d.styles.find("heading 1").unwrap();
        d.apply_style(MAIN, 0, 0, heading);
        let callout = d
            .add_character_style(
                "Callout",
                CharacterFormat::new(
                    CharProps {
                        bold: true,
                        size: 36,
                        ..Default::default()
                    },
                    (1 << 1) | (1 << 2),
                ),
            )
            .unwrap();
        d.forget_history();
        (d, heading, callout)
    }

    #[test]
    fn a_linked_paragraph_style_takes_the_character_style_and_unlinking_restores_it() {
        let (mut d, heading, callout) = linked_fixture();
        let own = d.styles.get(heading).chp;
        let pap = d.styles.get(heading).pap.clone();
        d.begin("Link", Selection::default());
        d.redefine_style_linked(heading, pap.clone(), own, callout);
        d.commit(Selection::default());
        let style = d.styles.get(heading);
        assert_eq!(style.character, callout);
        assert_eq!(style.own(), own, "the manual values are kept");
        assert_eq!(style.chp.size, 36);
        assert_eq!(
            style.chp.bold, !own.bold,
            "Bold switches the paragraph's value"
        );
        assert_eq!(
            style.chp.font, own.font,
            "what Callout leaves alone shows through"
        );
        let c = d.main.paras[0].chp_of_char(0);
        assert_eq!((c.size, c.bold), (36, !own.bold));
        assert_eq!(
            d.main.paras[1].chp_of_char(0).size,
            d.styles.get(0).chp.size
        );

        d.begin("Unlink", Selection::default());
        d.redefine_style_linked(heading, pap, own, 0);
        d.commit(Selection::default());
        assert_eq!(d.styles.get(heading).chp, own);
        assert_eq!(d.main.paras[0].chp_of_char(0).size, own.size);
        d.undo();
        assert_eq!(d.styles.get(heading).character, callout);
        assert_eq!(d.main.paras[0].chp_of_char(0).size, 36);
    }

    #[test]
    fn changing_a_character_style_reaches_linked_paragraph_styles_and_their_text() {
        let (mut d, heading, callout) = linked_fixture();
        let (pap, own) = (d.styles.get(heading).pap.clone(), d.styles.get(heading).chp);
        d.redefine_style_linked(heading, pap, own, callout);
        let format = CharacterFormat::new(
            CharProps {
                size: 48,
                color: Rgb(0xFF0000),
                ..Default::default()
            },
            (1 << 1) | (1 << 11),
        );
        assert!(d.redefine_character_style(callout, format));
        let style = d.styles.get(heading);
        assert_eq!((style.chp.size, style.chp.color), (48, Rgb(0xFF0000)));
        assert_eq!(style.chp.bold, own.bold, "Callout no longer sets Bold");
        let c = d.main.paras[0].chp_of_char(0);
        assert_eq!((c.size, c.color, c.bold), (48, Rgb(0xFF0000), own.bold));
    }

    #[test]
    fn deleting_a_linked_character_style_keeps_the_look_and_renumbers_other_links() {
        let (mut d, heading, callout) = linked_fixture();
        let quote = d
            .add_character_style(
                "Quote",
                CharacterFormat::new(
                    CharProps {
                        italic: true,
                        ..Default::default()
                    },
                    1 << 3,
                ),
            )
            .unwrap();
        let normal_pap = d.styles.get(0).pap.clone();
        let normal_own = d.styles.get(0).chp;
        d.redefine_style_linked(0, normal_pap, normal_own, quote);
        let (pap, own) = (d.styles.get(heading).pap.clone(), d.styles.get(heading).chp);
        d.redefine_style_linked(heading, pap, own, callout);
        let looked = d.styles.get(heading).chp;
        assert!(d.delete_character_style(callout));
        let style = d.styles.get(heading);
        assert_eq!((style.character, style.chp), (0, looked));
        assert_eq!(style.own(), looked);
        assert_eq!(d.styles.get(0).character, quote - 1, "Quote moved down");
        assert!(d.styles.get(0).chp.italic);
    }

    #[test]
    fn a_link_to_a_missing_style_is_dropped_and_a_good_one_decides_the_look() {
        let (mut d, heading, callout) = linked_fixture();
        d.styles.styles[heading as usize].character = 9;
        d.keep_in_bounds();
        assert_eq!(d.styles.get(heading).character, 0);
        let own = d.styles.get(heading).chp;
        d.styles.styles[heading as usize].character = callout;
        d.styles.styles[heading as usize].own_chp = own;
        d.keep_in_bounds();
        assert_eq!(d.styles.get(heading).chp.size, 36);
    }

    #[test]
    fn named_layer_inherits_font_and_keeps_unrelated_format_and_metadata() {
        let mut d = crate::text::from_plain_text("café emphasis");
        let c = CharProps {
            italic: true,
            color: Rgb(0x123456),
            ..Default::default()
        };
        let id = d
            .add_character_style("Emphasis", CharacterFormat::new(c, 1 << 3))
            .unwrap();
        d.main.paras[0].format_range(0, 5, &mut |c| {
            c.bold = true;
            c.link = 71;
            c.comment = 22;
        });
        d.apply_character_style(MAIN, Position::new(0, 0), Position::new(0, 5), id);
        let c = d.main.paras[0].chp_of_char(0);
        assert!(c.italic && c.bold);
        assert_eq!((c.link, c.comment), (71, 22));
        assert_eq!(c.color, Rgb::BLACK);
        let mut pap = d.styles.get(0).pap.clone();
        pap.left = 144;
        let base = CharProps {
            size: 36,
            font: d.font_id("Georgia"),
            ..d.styles.get(0).chp
        };
        d.redefine_style(0, pap, base);
        let c = d.main.paras[0].chp_of_char(0);
        assert_eq!((c.font, c.size), (base.font, 36));
        assert!(c.italic && c.bold);
        d.apply_character_style(MAIN, Position::new(0, 0), Position::new(0, 5), 0);
        let c = d.main.paras[0].chp_of_char(0);
        assert!(!c.italic && c.bold);
        assert_eq!((c.link, c.comment), (71, 22));
    }

    #[test]
    fn redefinition_and_deletion_cover_notes_headers_and_direct_overrides_and_undo() {
        let mut d = crate::text::from_plain_text("Main");
        let mut props = CharProps {
            italic: true,
            ..Default::default()
        };
        let id = d
            .add_character_style("Emphasis", CharacterFormat::new(props, 1 << 3))
            .unwrap();
        d.notes = crate::text::from_plain_text("Note").main;
        d.headers[0] = crate::text::from_plain_text("Header").main;
        for story in [MAIN, StoryId::Notes, StoryId::Header] {
            d.apply_character_style(story, Position::new(0, 0), Position::new(0, 3), id);
        }
        d.format_chars(MAIN, Position::new(0, 0), Position::new(0, 1), |c| {
            c.italic = false
        });
        d.commit(Selection::default());
        d.begin("Redefine", Selection::default());
        props.color = Rgb(0xCC2200);
        d.redefine_character_style(id, CharacterFormat::new(props, (1 << 3) | (1 << 11)));
        d.commit(Selection::default());
        for story in [MAIN, StoryId::Notes, StoryId::Header] {
            assert_eq!(d.story(story).paras[0].chp_at(0).color, props.color);
        }
        assert!(!d.main.paras[0].chp_of_char(0).italic);
        assert!(d.notes.paras[0].chp_at(0).italic);
        d.undo();
        assert_eq!(d.notes.paras[0].chp_at(0).color, Rgb::BLACK);
        d.redo();
        d.begin("Delete", Selection::default());
        d.delete_character_style(id);
        d.commit(Selection::default());
        assert_eq!(d.headers[0].paras[0].chp_at(0).character_style, 0);
        assert_eq!(d.headers[0].paras[0].chp_at(0).color, props.color);
        d.undo();
        assert_eq!(d.headers[0].paras[0].chp_at(0).character_style, id);
        assert_eq!(d.styles.character(id).unwrap().name, "Emphasis");
    }

    #[test]
    fn styled_text_joined_to_another_paragraph_keeps_its_look_as_overrides() {
        let mut d = crate::text::from_plain_text("One\nTwo");
        let heading = d.add_style(
            "Heading",
            0,
            None,
            Default::default(),
            CharProps {
                bold: true,
                ..Default::default()
            },
        );
        let italic = CharProps {
            italic: true,
            ..Default::default()
        };
        let id = d
            .add_character_style("Emphasis", CharacterFormat::new(italic, 1 << 3))
            .unwrap();
        d.apply_style(MAIN, 0, 0, heading);
        d.apply_character_style(MAIN, Position::new(0, 0), Position::new(0, 3), id);
        d.apply_style(MAIN, 1, 1, 0);
        d.commit(Selection::default());
        // The heading's bold run now sits in Normal, and that is its own.
        d.begin("Join", Selection::default());
        d.main.paras[0].pap.style = 0;
        let p = d.main.paras[0].clone();
        d.splice(MAIN, 0, 1, vec![p]);
        d.commit(Selection::default());
        let c = d.main.paras[0].chp_of_char(0);
        assert!(c.bold && c.italic);
        assert_eq!(c.direct_format, 1 << 2);
        d.begin("Redefine", Selection::default());
        let format = CharacterFormat::new(italic, (1 << 3) | (1 << 2));
        d.redefine_character_style(id, format);
        d.commit(Selection::default());
        assert!(d.main.paras[0].chp_of_char(0).bold);
    }

    #[test]
    fn deleting_a_style_leaves_later_styles_without_overrides() {
        let mut d = crate::text::from_plain_text("One");
        let italic = CharProps {
            italic: true,
            ..Default::default()
        };
        let bold = CharProps {
            bold: true,
            ..Default::default()
        };
        let first = d
            .add_character_style("First", CharacterFormat::new(italic, 1 << 3))
            .unwrap();
        let second = d
            .add_character_style("Second", CharacterFormat::new(bold, 1 << 2))
            .unwrap();
        d.apply_character_style(MAIN, Position::new(0, 0), Position::new(0, 3), second);
        d.commit(Selection::default());
        d.begin("Delete", Selection::default());
        d.delete_character_style(first);
        d.commit(Selection::default());
        let c = d.main.paras[0].chp_of_char(0);
        assert_eq!((c.character_style, c.direct_format), (first, 0));
    }

    #[test]
    fn replacing_a_font_leaves_styled_text_following_its_style() {
        let mut d = crate::text::from_plain_text("Main");
        let (old, new, later) = (d.font_id("Old"), d.font_id("New"), d.font_id("Later"));
        let props = |font| CharProps {
            font,
            ..Default::default()
        };
        let id = d
            .add_character_style("Fancy", CharacterFormat::new(props(old), 1))
            .unwrap();
        d.headers[0] = crate::text::from_plain_text("Header").main;
        for story in [MAIN, StoryId::Header] {
            d.apply_character_style(story, Position::new(0, 0), Position::new(0, 4), id);
        }
        d.commit(Selection::default());
        d.begin("Replace Fonts", Selection::default());
        d.replace_font(&[old], new);
        d.commit(Selection::default());
        for story in [MAIN, StoryId::Header] {
            let c = d.story(story).paras[0].chp_of_char(0);
            assert_eq!((c.font, c.direct_format), (new, 0));
        }
        d.begin("Redefine", Selection::default());
        d.redefine_character_style(id, CharacterFormat::new(props(later), 1));
        d.commit(Selection::default());
        for story in [MAIN, StoryId::Header] {
            assert_eq!(d.story(story).paras[0].chp_of_char(0).font, later);
        }
    }

    #[test]
    fn replacing_a_color_changes_text_styles_and_linked_styles_and_undoes() {
        let (old, new) = (Rgb(0x336699), Rgb::BLACK);
        let mut d = crate::text::from_plain_text("Plain Styled\nHeading");
        let id = d
            .add_character_style(
                "Brand",
                CharacterFormat::new(
                    CharProps {
                        color: old,
                        ..Default::default()
                    },
                    1 << 11,
                ),
            )
            .unwrap();
        d.format_chars(MAIN, Position::new(0, 0), Position::new(0, 5), |c| {
            c.color = old
        });
        d.apply_character_style(MAIN, Position::new(0, 6), Position::new(0, 12), id);
        let heading = d.styles.find("heading 1").unwrap();
        d.apply_style(MAIN, 1, 1, heading);
        let (pap, mut own) = (d.styles.get(heading).pap.clone(), d.styles.get(heading).chp);
        own.color = old;
        d.redefine_style_linked(heading, pap, own, id);
        d.commit(Selection::default());
        let before = d.main.clone();

        d.begin("Delete Color", Selection::default());
        d.replace_color(old, new);
        d.commit(Selection::default());
        for (para, at) in [(0, 0), (0, 6), (1, 0)] {
            assert_eq!(d.main.paras[para].chp_of_char(at).color, new);
            assert_eq!(d.main.paras[para].chp_of_char(at).direct_format, 0);
        }
        assert_eq!(d.styles.character(id).unwrap().format.props.color, new);
        assert_eq!(d.styles.get(heading).own().color, new);
        // Unlinking brings back the style's own formatting, which changed too.
        let (pap, own) = (
            d.styles.get(heading).pap.clone(),
            d.styles.get(heading).own(),
        );
        d.redefine_style_linked(heading, pap, own, 0);
        assert_eq!(d.styles.get(heading).chp.color, new);
        d.undo();
        d.undo();
        assert_eq!(d.main, before);
        assert_eq!(d.styles.character(id).unwrap().format.props.color, old);
        assert_eq!(d.styles.get(heading).own().color, old);
    }

    #[test]
    fn the_first_hyperlink_defines_the_shared_look_and_boundaries_drop_the_named_layer() {
        let mut d = crate::text::from_plain_text("first second");
        let link = crate::link::intern(crate::Link::new("https://example.com"));
        d.set_link(
            MAIN,
            Position::new(0, 0),
            Position::new(0, 5),
            link,
            Rgb(0xCC2200),
        );
        d.set_link(
            MAIN,
            Position::new(0, 6),
            Position::new(0, 12),
            link,
            Rgb(0xCC2200),
        );
        let id = d.styles.find_character("Hyperlink").unwrap();
        for at in [0, 6] {
            let c = d.main.paras[0].chp_of_char(at);
            assert_eq!((c.character_style, c.color), (id, Rgb(0xCC2200)));
        }
        let props = CharProps {
            color: Rgb(0x228844),
            underline: crate::Underline::Single,
            ..Default::default()
        };
        d.redefine_character_style(id, CharacterFormat::new(props, (1 << 4) | (1 << 11)));
        assert_eq!(d.main.paras[0].chp_of_char(6).color, props.color);
        d.set_link(
            MAIN,
            Position::new(0, 0),
            Position::new(0, 5),
            link,
            Rgb(0x0000EE),
        );
        assert_eq!(d.main.paras[0].chp_of_char(0).color, Rgb(0x0000EE));
        assert_eq!(d.main.paras[0].chp_of_char(6).color, props.color);
        let outside = d.main.paras[0].chp_at(5);
        assert_eq!((outside.link, outside.character_style), (0, 0));
    }

    #[test]
    fn invalid_names_and_ids_do_not_change_the_document() {
        let mut d = Document::new();
        assert!(
            d.add_character_style("  ", CharacterFormat::default())
                .is_none()
        );
        assert!(
            d.add_character_style(&"x".repeat(256), CharacterFormat::default())
                .is_none()
        );
        assert_eq!(d.undo_label(), None);
        d.apply_character_style(MAIN, Position::new(0, 0), Position::new(0, 0), u16::MAX);
        assert!(!d.redefine_character_style(u16::MAX, CharacterFormat::default()));
        assert!(!d.delete_character_style(u16::MAX));
        assert_eq!(d.undo_label(), None);
    }
}
