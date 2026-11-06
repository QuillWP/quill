//! Font names given as PostScript names. Rich text copied from other Mac
//! apps names its fonts that way ("HelveticaNeue-Bold" for Helvetica Neue
//! in bold), and so do some RTF files. Quill lists fonts by family, so such a
//! name looked like a font that is not installed: the text showed in a
//! stand-in and opening the file warned about it. Quill turns them into the
//! family name with bold or italic set as the name says. It also knows
//! which fonts are installed, for text pasted in a font that is not.

use quill_core::{CharProps, Document};
use quill_layout::FontVariant;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

type Names = HashMap<String, (String, FontVariant)>;

struct Installed {
    postscript: Names,
    families: HashSet<String>,
}

// What is installed, set when the font list is built or changes. Empty
// until then, which leaves names as they are.
static INSTALLED: RwLock<Option<Installed>> = RwLock::new(None);

/// Remembers the fonts now installed.
pub fn set(catalog: &quill_layout::Catalog) {
    *INSTALLED.write().unwrap_or_else(|e| e.into_inner()) = Some(Installed {
        postscript: catalog.postscript_names(),
        families: catalog.family_keys(),
    });
}

/// Whether a font family of this name is installed. True until the font
/// list is known, so nothing is thrown away on a guess.
pub fn is_installed(name: &str) -> bool {
    let installed = INSTALLED.read().unwrap_or_else(|e| e.into_inner());
    installed
        .as_ref()
        .is_none_or(|i| i.families.contains(&name.to_lowercase()))
}

/// Rewrites `doc`'s fonts that are named by PostScript name to their
/// family, everywhere they are used.
pub fn use_family_names(doc: &mut Document) {
    let installed = INSTALLED.read().unwrap_or_else(|e| e.into_inner());
    if let Some(installed) = installed.as_ref() {
        rename(doc, &installed.postscript);
    }
}

fn rename(doc: &mut Document, names: &Names) {
    // Old font id to (new id, style).
    let mut map: HashMap<u16, (u16, FontVariant)> = HashMap::new();
    for id in 0..doc.fonts.len() {
        let Ok(id) = u16::try_from(id) else {
            break;
        };
        if let Some((family, variant)) = names.get(&doc.font_name(id).to_lowercase()) {
            let family = family.clone();
            let variant = *variant;
            map.insert(id, (doc.font_id(&family), variant));
        }
    }
    if map.is_empty() {
        return;
    }
    let fix = |chp: &mut CharProps| {
        if let Some((font, variant)) = map.get(&chp.font) {
            chp.font = *font;
            chp.bold |= variant.bold;
            chp.italic |= variant.italic;
        }
    };
    let stories = std::iter::once(&mut doc.main)
        .chain(doc.headers.iter_mut())
        .chain([&mut doc.notes, &mut doc.annotations]);
    for story in stories {
        for p in &mut story.paras {
            p.runs.iter_mut().for_each(|r| fix(&mut r.chp));
            fix(&mut p.mark_chp);
        }
    }
    doc.styles.styles.iter_mut().for_each(|s| fix(&mut s.chp));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postscript_names_become_the_family_in_its_style() {
        let mut names = Names::new();
        let bold = FontVariant {
            bold: true,
            italic: false,
        };
        names.insert("helveticaneue-bold".into(), ("Helvetica Neue".into(), bold));
        let mut doc = Document::new();
        let ps = doc.font_id("HelveticaNeue-Bold");
        let georgia = doc.font_id("Georgia");
        let base = doc.styles.get(0).chp;
        doc.main.paras[0].insert(
            0,
            "Plain",
            CharProps {
                font: georgia,
                ..base
            },
        );
        doc.main.paras[0].insert(5, " pasted", CharProps { font: ps, ..base });
        rename(&mut doc, &names);
        let runs = &doc.main.paras[0].runs;
        assert_eq!(doc.font_name(runs[0].chp.font), "Georgia");
        assert!(!runs[0].chp.bold);
        assert_eq!(doc.font_name(runs[1].chp.font), "Helvetica Neue");
        assert!(runs[1].chp.bold);
    }
}
