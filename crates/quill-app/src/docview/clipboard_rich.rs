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

    /// Text from another application can name a font that is not on this
    /// computer, such as a web font a browser downloaded for one page.
    /// Kept, it shows in a stand-in and warns on every open, so it takes
    /// `font`, the font where it is going, instead. Returns the names of the
    /// fonts the text had been using. Copies within Quill keep their fonts.
    pub fn drop_missing_fonts(
        &mut self,
        font: &str,
        installed: impl Fn(&str) -> bool,
    ) -> Vec<String> {
        if !self.external {
            return Vec::new();
        }
        let mut used = BTreeSet::new();
        for p in self
            .fragment
            .paras
            .iter()
            .chain(self.fragment.notes.iter().flat_map(|note| &note.paras))
        {
            used.extend(p.runs.iter().filter(|r| r.len > 0).map(|r| r.chp.font));
        }
        let mut dropped = Vec::new();
        for (id, name) in self.fonts.iter_mut().enumerate() {
            if installed(name) {
                continue;
            }
            if u16::try_from(id).is_ok_and(|id| used.contains(&id)) && !dropped.contains(name) {
                dropped.push(name.clone());
            }
            *name = font.to_string();
        }
        dropped
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
        let mut used = BTreeSet::new();
        for p in self
            .fragment
            .paras
            .iter()
            .chain(self.fragment.notes.iter().flat_map(|note| &note.paras))
        {
            used.insert(p.pap.style);
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
        let mut fragment = self.fragment.clone();
        for p in fragment
            .paras
            .iter_mut()
            .chain(fragment.notes.iter_mut().flat_map(|note| &mut note.paras))
        {
            p.pap.style = styles.get(&p.pap.style).copied().unwrap_or(0);
            font(&mut p.mark_chp);
            for run in &mut p.runs {
                font(&mut run.chp);
            }
        }
        fragment
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::Selection;

    #[test]
    fn pasted_text_gives_up_fonts_that_are_not_installed() {
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
        assert!(own.drop_missing_fonts("Times", installed).is_empty());

        let mut rich = RichContent::from_document(source, None);
        assert_eq!(rich.drop_missing_fonts("Times", installed), ["Google Sans"]);
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
    fn rtf_copied_from_a_browser_loses_its_web_font() {
        let rtf = br"{\rtf1\ansi{\fonttbl\f0\fnil Google Sans;}\f0\fs24 Hello}";
        let doc = quill_formats::rtf::read(rtf).unwrap();
        let mut rich = RichContent::from_document(doc, Some("Hello"));
        let dropped = rich.drop_missing_fonts("Georgia", |name| name != "Google Sans");
        assert_eq!(dropped, ["Google Sans"]);
        let mut target = Document::new();
        target.begin("Paste", Selection::default());
        let pasted = rich.prepare(&mut target);
        let chp = pasted.paras[0].chp_of_char(0);
        assert_eq!(target.font_name(chp.font), "Georgia");
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
