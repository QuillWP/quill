//! A selection plus the font/style tables its numeric references belong to.

use quill_core::para::Paragraph;
use quill_core::props::CharProps;
use quill_core::styles::StyleSheet;
use quill_core::{Document, Fragment, Position, StoryId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub struct RichContent {
    pub fragment: Fragment,
    fonts: Vec<String>,
    styles: StyleSheet,
    /// Copied from another application, not from a Quill document.
    external: bool,
}

impl RichContent {
    pub fn new(source: &Document, fragment: Fragment) -> Self {
        Self {
            fragment,
            fonts: source.fonts.clone(),
            styles: source.styles.clone(),
            external: false,
        }
    }

    /// Only fonts actually used by an external selection need a decision.
    pub fn missing_fonts(&self, installed: impl Fn(&str) -> bool) -> Vec<String> {
        if !self.external {
            return Vec::new();
        }
        let mut used = BTreeSet::new();
        let mut fragment = self.fragment.clone();
        each_para(&mut fragment, &mut |p| {
            used.extend(p.runs.iter().filter(|r| r.len > 0).map(|r| r.chp.font));
        });
        let mut missing = Vec::new();
        for id in used {
            if let Some(name) = self.fonts.get(id as usize)
                && !installed(name)
                && !missing.contains(name)
            {
                missing.push(name.clone());
            }
        }
        missing
    }

    /// Ask before changing the selection. None cancels the entire paste;
    /// false keeps the font names, and true replaces only the missing ones.
    pub fn resolve_missing_fonts(
        &mut self,
        font: &str,
        installed: impl Fn(&str) -> bool,
        choose: impl FnOnce(&[String], &str) -> Option<bool>,
    ) -> Option<Vec<String>> {
        let missing = self.missing_fonts(installed);
        if missing.is_empty() || !choose(&missing, font)? {
            return Some(Vec::new());
        }
        for name in &mut self.fonts {
            if missing.contains(name) {
                *name = font.to_string();
            }
        }
        Some(missing)
    }

    pub fn from_document(doc: Document, plain: Option<&str>) -> Self {
        let last = doc.main.paras.len() - 1;
        let mut frag = doc.copy(
            StoryId::Main,
            Position::new(0, 0),
            Position::new(last, doc.main.paras[last].len()),
        );
        frag.ends_with_mark = plain.is_some_and(|text| text.ends_with(['\n', '\r']));
        if let Some(plain) = plain {
            let plain = plain.replace("\r\n", "\n").replace('\r', "\n");
            if frag.paras.len() > 1 && frag.paras.last().is_some_and(Paragraph::is_empty) {
                let mut trimmed = frag.clone();
                trimmed.paras.pop();
                if trimmed.plain_text() == plain {
                    frag = trimmed;
                }
            }
        }
        Self {
            external: true,
            ..Self::new(&doc, frag)
        }
    }

    pub fn document(&self) -> Document {
        let mut doc = Document::new();
        doc.fonts = self.fonts.clone();
        doc.styles = self.styles.clone();
        // A fragment that starts in a table goes into an ordinary paragraph:
        // inside a table cell, pasting would turn the table into text.
        let table = self.fragment.paras.first().is_some_and(|p| p.pap.in_table);
        if let Some(first) = self.fragment.paras.first()
            && !table
        {
            doc.main.paras[0] = Paragraph::new(first.pap.clone(), first.mark_chp);
        }
        doc.insert_fragment(StoryId::Main, Position::new(0, 0), &self.fragment);
        // The paragraph a table needs after it is not part of the copy.
        let trailing = if table {
            doc.main.paras.last().is_some_and(|p| !p.pap.in_table)
        } else {
            self.fragment.ends_with_mark
        };
        if trailing && doc.main.paras.len() > 1 {
            doc.main.paras.pop();
        }
        doc.forget_history();
        doc
    }

    /// Call inside the paste undo transaction. Existing styles are never
    /// overwritten; conflicting names receive a reusable Pasted variant.
    pub fn prepare(&self, target: &mut Document) -> Fragment {
        let fonts: Vec<_> = self.fonts.iter().map(|name| target.font_id(name)).collect();
        let font = |chp: &mut CharProps| {
            chp.font = fonts.get(chp.font as usize).copied().unwrap_or(0);
        };
        let mut fragment = self.fragment.clone();
        let mut used = BTreeSet::new();
        let mut character_ids = BTreeSet::new();
        each_para(&mut fragment, &mut |p| {
            used.insert(p.pap.style);
            for c in p.runs.iter().map(|r| &r.chp).chain([&p.mark_chp]) {
                character_ids.insert(c.character_style);
            }
        });
        let mut characters = BTreeMap::new();
        for id in character_ids {
            let Some(source) = self.styles.character(id) else {
                continue;
            };
            let mapped = target.import_character_style(source, &self.fonts, true);
            characters.insert(id, mapped);
        }
        let mut styles = BTreeMap::new();
        for id in used {
            let source = self.styles.get(id);
            let mut pap = source.pap.clone();
            pap.style = 0;
            let mut chp = source.chp;
            font(&mut chp);
            let mut name = source.name.clone();
            let mut suffix = 0;
            let mapped = loop {
                if let Some(existing) = target.styles.find(&name) {
                    let other = target.styles.get(existing);
                    let mut other_pap = other.pap.clone();
                    other_pap.style = 0;
                    if other_pap == pap && other.chp == chp {
                        break existing;
                    }
                    suffix += 1;
                    name = if suffix == 1 {
                        format!("{} (Pasted)", source.name)
                    } else {
                        format!("{} (Pasted {suffix})", source.name)
                    };
                } else if target.styles.styles.len() < u16::MAX as usize {
                    break target.add_style(&name, 0, None, pap, chp);
                } else {
                    break 0;
                }
            };
            styles.insert(id, mapped);
        }
        each_para(&mut fragment, &mut |p| {
            p.pap.style = styles.get(&p.pap.style).copied().unwrap_or(0);
            let mut remap = |c: &mut CharProps| {
                font(c);
                c.character_style = characters.get(&c.character_style).copied().unwrap_or(0);
                if c.character_style == 0 {
                    c.direct_format = 0;
                }
            };
            remap(&mut p.mark_chp);
            // Two fonts can arrive as one, so runs may need to merge.
            let len = p.len();
            p.format_range(0, len, &mut remap);
        });
        fragment
    }
}

/// Every paragraph a fragment carries: its own, its notes', and the headers
/// and footers that come with the section breaks in it.
fn each_para(fragment: &mut Fragment, f: &mut impl FnMut(&mut Paragraph)) {
    for p in fragment
        .paras
        .iter_mut()
        .chain(fragment.notes.iter_mut().flat_map(|note| &mut note.paras))
    {
        if let Some(headers) = p.section.as_mut().and_then(|s| s.headers.as_mut()) {
            for story in std::sync::Arc::make_mut(headers) {
                story.paras.iter_mut().for_each(&mut *f);
            }
        }
        f(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::Selection;

    #[test]
    fn character_style_name_conflicts_preserve_both_definitions_and_reuse_pasted_styles() {
        use quill_core::character_styles::CharacterFormat;
        let mut source = quill_core::text::from_plain_text("Styled");
        let props = CharProps {
            italic: true,
            ..Default::default()
        };
        let id = source
            .add_character_style("Emphasis", CharacterFormat::new(props, 1 << 3))
            .unwrap();
        source.apply_character_style(StoryId::Main, Position::new(0, 0), Position::new(0, 6), id);
        let fragment = source.copy(StoryId::Main, Position::new(0, 0), Position::new(0, 6));
        let rich = RichContent::new(&source, fragment);
        let mut target = Document::new();
        let original = target
            .add_character_style(
                "Emphasis",
                CharacterFormat::new(
                    CharProps {
                        bold: true,
                        ..Default::default()
                    },
                    1 << 2,
                ),
            )
            .unwrap();
        target.commit(Selection::default());
        target.begin("Paste", Selection::default());
        let fragment = rich.prepare(&mut target);
        let mapped = fragment.paras[0].chp_of_char(0).character_style;
        assert_ne!(mapped, original);
        assert_eq!(
            target.styles.character(original).unwrap().format.mask,
            1 << 2
        );
        assert_eq!(target.styles.character(mapped).unwrap().format.mask, 1 << 3);
        let count = target.styles.characters.len();
        assert_eq!(
            rich.prepare(&mut target).paras[0]
                .chp_of_char(0)
                .character_style,
            mapped
        );
        assert_eq!(target.styles.characters.len(), count);
        target.commit(Selection::default());
        target.undo();
        assert_eq!(target.styles.characters.len(), 1);
    }

    #[test]
    fn replacing_missing_fonts_preserves_installed_fonts_and_other_formatting() {
        let installed = |name: &str| name != "Google Sans";
        let mut source = Document::new();
        let web = source.font_id("Google Sans");
        let georgia = source.font_id("Georgia");
        let base = source.styles.get(0).chp;
        let bold_web = quill_core::CharProps {
            font: web,
            bold: true,
            ..base
        };
        let plain = quill_core::CharProps {
            font: georgia,
            ..base
        };
        source.main.paras[0].insert(0, "web", bold_web);
        source.main.paras[0].insert(3, "kept", plain);
        let fragment = source.copy(
            StoryId::Main,
            Position::new(0, 0),
            Position::new(0, source.main.paras[0].len()),
        );

        // A copy within Quill keeps its fonts.
        let mut own = RichContent::new(&source, fragment);
        assert!(
            own.resolve_missing_fonts("Times", installed, |_, _| panic!("internal copy"))
                .unwrap()
                .is_empty()
        );

        let mut rich = RichContent::from_document(source, None);
        assert_eq!(
            rich.resolve_missing_fonts("Times", installed, |_, _| Some(true))
                .unwrap(),
            ["Google Sans"]
        );
        let mut target = Document::new();
        target.begin("Paste", Selection::default());
        let pasted = rich.prepare(&mut target);
        let c = pasted.paras[0].chp_of_char(0);
        assert_eq!(target.font_name(c.font), "Times");
        assert!(c.bold, "only the font changes");
        let c = pasted.paras[0].chp_of_char(3);
        assert_eq!(target.font_name(c.font), "Georgia");
    }

    #[test]
    fn external_rtf_replaces_its_web_font_after_the_choice() {
        let rtf = br"{\rtf1\ansi{\fonttbl\f0\fnil Google Sans;}\f0\fs24 Hello}";
        let doc = quill_formats::rtf::read(rtf).unwrap();
        let mut rich = RichContent::from_document(doc, Some("Hello"));
        let dropped = rich
            .resolve_missing_fonts("Georgia", |name| name != "Google Sans", |_, _| Some(true))
            .unwrap();
        assert_eq!(dropped, ["Google Sans"]);
        let mut target = Document::new();
        target.begin("Paste", Selection::default());
        let pasted = rich.prepare(&mut target);
        let chp = pasted.paras[0].chp_of_char(0);
        assert_eq!(target.font_name(chp.font), "Georgia");
    }

    #[test]
    fn missing_font_choice_keeps_names_or_cancels_without_replacing_them() {
        let bytes = br"{\rtf1\ansi{\fonttbl{\f0 Garamond;}{\f1 Georgia;}{\f2 Unused;}}\f0\b Missing\f1\b0  installed}";
        for choice in [Some(false), None] {
            let doc = quill_formats::rtf::read(bytes).unwrap();
            let mut rich = RichContent::from_document(doc, None);
            let before = rich.fonts.clone();
            let answer = rich.resolve_missing_fonts(
                "Times",
                |name| name == "Georgia",
                |missing, replacement| {
                    assert_eq!(missing, ["Garamond"]);
                    assert_eq!(replacement, "Times");
                    choice
                },
            );
            assert_eq!(answer, choice.map(|_| Vec::<String>::new()));
            assert_eq!(rich.fonts, before);
            let mut target = Document::new();
            let fragment = rich.prepare(&mut target);
            let chp = fragment.paras[0].chp_of_char(0);
            assert_eq!(target.font_name(chp.font), "Garamond");
            assert!(chp.bold);
        }
    }

    #[test]
    fn two_missing_fonts_replaced_by_one_become_one_run() {
        let bytes = br"{\rtf1\ansi{\fonttbl{\f0 Garamond;}{\f1 Aptos;}}\f0 One\f1  two}";
        let doc = quill_formats::rtf::read(bytes).unwrap();
        let mut rich = RichContent::from_document(doc, None);
        rich.resolve_missing_fonts("Georgia", |_| false, |_, _| Some(true))
            .unwrap();
        let fragment = rich.prepare(&mut Document::new());
        assert_eq!(fragment.paras[0].runs.len(), 1);
    }

    /// Found by fuzzing: a pasted section break's footer kept the source's
    /// style number, past the end of this document's styles, and Save As
    /// .docx then crashed looking it up.
    #[test]
    fn a_pasted_section_breaks_footer_uses_this_documents_styles_and_fonts() {
        let mut stylesheet = String::from(r"{\s0 Normal;}");
        for k in 1..40 {
            stylesheet.push_str(&format!(r"{{\s{k} Filler {k};}}"));
        }
        stylesheet.push_str(r"{\s40\i Footer Style;}");
        let rtf = format!(
            r"{{\rtf1\ansi{{\fonttbl{{\f0 Times;}}{{\f1 Garamond;}}}}{{\stylesheet{stylesheet}}}{body}}}",
            body = r"\pard\plain One\sect\sectd{\footer\pard\plain\s40\f1 Footer\par}\pard\plain Two\sect\sectd\pard\plain Three\par"
        );
        let doc = quill_formats::rtf::read(rtf.as_bytes()).unwrap();
        let rich = RichContent::from_document(doc, None);
        let fonts = rich.missing_fonts(|name| name == "Times");
        assert_eq!(fonts, ["Garamond"], "the footer's font is asked about");
        let mut target = Document::new();
        target.begin("Paste", Selection::default());
        let fragment = rich.prepare(&mut target);
        target.insert_fragment(StoryId::Main, Position::new(0, 0), &fragment);
        target.commit(Selection::default());
        let footer = target
            .main
            .paras
            .iter()
            .find_map(|p| p.section.as_ref()?.headers.as_ref())
            .and_then(|h| h.iter().find(|s| s.paras[0].text == "Footer"))
            .expect("the pasted footer");
        let p = &footer.paras[0];
        assert_eq!(target.styles.get(p.pap.style).name, "Footer Style");
        assert_eq!(target.font_name(p.chp_of_char(0).font), "Garamond");
        quill_formats::docx::read(&quill_formats::docx::write(&target, 0)).unwrap();
    }

    #[test]
    fn installed_fonts_paste_without_asking() {
        let doc = quill_core::text::from_plain_text("Installed");
        let mut rich = RichContent::from_document(doc, None);
        assert_eq!(
            rich.resolve_missing_fonts("Georgia", |_| true, |_, _| panic!("installed font")),
            Some(Vec::new())
        );
    }

    #[test]
    fn cross_document_paste_remaps_fonts_and_reuses_conflicting_styles() {
        let mut source = Document::new();
        let font = source.font_id("Clipboard Test Font");
        source.styles.styles[0].chp.font = font;
        let chp = source.styles.get(0).chp;
        source.main.paras[0].insert(0, "Café \u{2764}\u{fe0f}", chp);
        let fragment = source.copy(
            StoryId::Main,
            Position::new(0, 0),
            Position::new(0, source.main.paras[0].len()),
        );
        let rich = RichContent::new(&source, fragment);
        let mut target = Document::new();
        target.font_id("A different font at the same numeric index");
        let original = target.styles.get(0).clone();
        target.begin("Paste", Selection::default());
        let pasted = rich.prepare(&mut target);
        let chp = pasted.paras[0].chp_of_char(0);
        assert_eq!(target.font_name(chp.font), "Clipboard Test Font");
        assert_eq!(target.styles.get(0), &original);
        let count = target.styles.styles.len();
        assert!(
            target
                .styles
                .get(pasted.paras[0].pap.style)
                .name
                .contains("Pasted")
        );
        let _ = rich.prepare(&mut target);
        assert_eq!(target.styles.styles.len(), count);
        let end = target.insert_fragment(StoryId::Main, Position::new(0, 0), &pasted);
        target.commit(Selection::caret(end));
        assert_eq!(target.main.paras[0].text, "Café \u{2764}\u{fe0f}");
        target.undo();
        assert!(target.main.paras[0].is_empty());
        assert_eq!(target.styles.styles.len(), count - 1);
    }

    #[test]
    fn copied_table_keeps_its_rows_for_other_programs() {
        let mut d = Document::new();
        d.insert_table(StoryId::Main, Position::new(0, 0), 2, 2, 6000);
        let chp = d.styles.get(0).chp;
        for (para, text) in [(0, "a"), (1, "b"), (3, "c"), (4, "")] {
            d.main.paras[para].insert(0, text, chp);
        }
        let last = d.main.paras.len() - 1;
        let frag = d.copy(
            StoryId::Main,
            Position::new(0, 0),
            Position::new(last, d.main.paras[last].len()),
        );
        assert_eq!(frag.plain_text(), "a\tb\nc\t\n");
        let doc = RichContent::new(&d, frag).document();
        let html = quill_formats::html::write_clipboard(&doc);
        assert_eq!(html.matches("<tr>").count(), 2);
        assert_eq!(html.matches("<td").count(), 4);
        let rtf =
            String::from_utf8_lossy(&quill_formats::rtf::write_clipboard(&doc, false)).into_owned();
        assert_eq!(rtf.matches("\\trowd").count(), 2);
        assert!(rtf.contains("\\cell"));
    }
}
