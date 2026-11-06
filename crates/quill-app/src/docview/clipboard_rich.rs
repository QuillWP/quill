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
}

impl RichContent {
    pub fn new(source: &Document, fragment: Fragment) -> Self {
        Self {
            fragment,
            fonts: source.fonts.clone(),
            styles: source.styles.clone(),
        }
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
        Self::new(&doc, frag)
    }

    pub fn document(&self) -> Document {
        let mut doc = Document::new();
        doc.fonts = self.fonts.clone();
        doc.styles = self.styles.clone();
        if let Some(first) = self.fragment.paras.first() {
            doc.main.paras[0] = Paragraph::new(first.pap.clone(), first.mark_chp);
        }
        doc.insert_fragment(StoryId::Main, Position::new(0, 0), &self.fragment);
        if self.fragment.ends_with_mark && doc.main.paras.len() > 1 {
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
}
