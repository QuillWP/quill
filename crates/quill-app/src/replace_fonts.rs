//! Format > Replace Missing Fonts…: one row for each font the document uses
//! that is not installed, with an installed font to use from now on.

use crate::dialog::Dialog;
use crate::docview::DocView;
use quill_layout::FontFamilyInfo;

const ROW_IDS: [&str; 10] = [
    "font0", "font1", "font2", "font3", "font4", "font5", "font6", "font7", "font8", "font9",
];
const ROW: f32 = 26.0;

/// The fonts a page of the dialog is for, and what comes after it.
#[derive(Clone, Debug)]
pub struct ReplaceState {
    names: Vec<String>,
    /// Each font still to come and the one shown in its place.
    rest: Vec<(String, String)>,
    families: Vec<FontFamilyInfo>,
}

impl ReplaceState {
    /// The next page, once this one is done.
    pub fn next_page(&self) -> Option<(Dialog, ReplaceState)> {
        (!self.rest.is_empty()).then(|| dialog(&self.rest, &self.families))
    }
}

fn more_line(more: usize) -> String {
    match more {
        1 => "1 more missing font after these.".to_string(),
        n => format!("{n} more missing fonts after these."),
    }
}

/// The dialog's first page for `missing`, each a font and the one shown in
/// its place. A page holds ten; the rest follow it, one page at a time.
pub fn dialog(missing: &[(String, String)], families: &[FontFamilyInfo]) -> (Dialog, ReplaceState) {
    let rows = missing.len().min(ROW_IDS.len());
    let more = missing.len() - rows;
    let height = 92.0 + rows as f32 * ROW + if more > 0 { 20.0 } else { 0.0 };
    let mut d = Dialog::new("Replace Missing Fonts", 440.0, height);
    d.text(
        "Use these installed fonts in place of the missing ones:",
        12.0,
        12.0,
    );
    let mut names = Vec::new();
    for (row, (font, shown)) in missing.iter().take(rows).enumerate() {
        let y = 38.0 + row as f32 * ROW;
        let sel = families
            .iter()
            .position(|f| f.name.eq_ignore_ascii_case(shown))
            .unwrap_or(0);
        d.text(font, 12.0, y + 3.0);
        d.font_popup(ROW_IDS[row], families, sel, 210.0, y, 218.0);
        names.push(font.clone());
    }
    if more > 0 {
        d.text(&more_line(more), 12.0, 44.0 + rows as f32 * ROW);
    }
    let label = if more > 0 {
        "Replace and Next"
    } else {
        "Replace"
    };
    d.button("ok", label, 310.0, height - 34.0, 118.0, true);
    d.button("cancel", "Cancel", 234.0, height - 34.0, 68.0, false);
    let st = ReplaceState {
        names,
        rest: missing[rows..].to_vec(),
        families: families.to_vec(),
    };
    (d, st)
}

/// Replace each font on this page with the one chosen for it.
pub fn apply(st: &ReplaceState, d: &Dialog, view: &mut DocView) -> bool {
    let pairs: Vec<(String, String)> = st
        .names
        .iter()
        .zip(ROW_IDS)
        .map(|(font, id)| (font.clone(), d.selected_text(id)))
        .collect();
    view.replace_fonts(&pairs);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::{CharProps, Document};

    #[test]
    fn missing_fonts_come_ten_to_a_page() {
        let missing: Vec<(String, String)> = (0..23)
            .map(|i| (format!("Gone {i}"), "Times".to_string()))
            .collect();
        let (d, st) = dialog(&missing, &[]);
        assert_eq!(st.names.len(), 10);
        assert!(d.item_rect("font9").is_some());
        let (d, st) = st.next_page().unwrap();
        assert_eq!(st.names[0], "Gone 10");
        assert_eq!(st.rest.len(), 3);
        assert!(d.item_rect("font9").is_some());
        let (_, st) = st.next_page().unwrap();
        assert_eq!(st.names.len(), 3);
        assert!(st.next_page().is_none());
        assert_eq!(more_line(1), "1 more missing font after these.");
        assert_eq!(more_line(13), "13 more missing fonts after these.");
    }

    #[test]
    fn a_replaced_font_changes_the_text_and_undoes() {
        let mut doc = Document::new();
        let gone = doc.font_id("Nosuch Sans");
        let chp = CharProps {
            font: gone,
            ..doc.styles.get(0).chp
        };
        doc.main.paras[0].insert(0, "Hello", chp);
        let mut view = crate::docview::DocView::new(doc, String::new());
        let name = |v: &crate::docview::DocView| {
            let font = v.doc.main.paras[0].runs[0].chp.font;
            v.doc.font_name(font).to_string()
        };
        view.replace_fonts(&[("nosuch sans".into(), "Georgia".into())]);
        assert_eq!(name(&view), "Georgia");
        assert!(view.doc.is_modified());
        let mut clip = crate::docview::Clipboard::new();
        view.exec(crate::commands::Cmd::Undo, false, &mut clip);
        assert_eq!(name(&view), "Nosuch Sans");
    }
}
