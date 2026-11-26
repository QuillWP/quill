//! The paragraph styles shown by controls scoped to "This Document".

use quill_core::para::chars;
use quill_core::props::StyleId;
use quill_core::{Document, StoryId};

/// Normal, styles in use, styles the user made, and useful built-in styles.
/// `keep` remains visible even when it is on an empty active paragraph that
/// does not otherwise count as document content.
pub fn paragraph_styles(doc: &Document, keep: Option<StyleId>) -> Vec<StyleId> {
    let mut used = vec![false; doc.styles.styles.len()];
    let mut header = false;
    let mut footer = false;
    let mut headers = doc.header_sets();
    headers.dedup_by_key(|(source, _)| *source);
    for (_, stories) in headers {
        for (kind, story) in StoryId::HEADERS.into_iter().zip(stories) {
            for para in story.paras.iter().filter(|p| !p.is_empty()) {
                used[para.pap.style as usize] = true;
                if kind.is_footer() {
                    footer = true;
                } else {
                    header = true;
                }
            }
        }
    }
    for para in &doc.main.paras {
        used[para.pap.style as usize] = true;
    }
    for story in [&doc.notes, &doc.annotations] {
        for para in story.paras.iter().filter(|p| !p.is_empty()) {
            used[para.pap.style as usize] = true;
        }
    }
    let last_heading = used
        .iter()
        .enumerate()
        .filter(|&(_, &used)| used)
        .filter_map(|(id, _)| doc.styles.heading_level(id as StyleId))
        .max()
        .unwrap_or(1)
        .saturating_add(2)
        .clamp(3, 9);
    let footnotes = doc
        .main
        .paras
        .iter()
        .any(|p| p.text.contains(chars::FOOTNOTE));
    doc.styles
        .styles
        .iter()
        .enumerate()
        .filter_map(|(id, style)| {
            let id = id as StyleId;
            let visible = if id == 0 || Some(id) == keep || !style.builtin || used[id as usize] {
                true
            } else {
                match style.name.as_str() {
                    "header" => header,
                    "footer" => footer,
                    "footnote reference" | "footnote text" => footnotes,
                    _ if doc.styles.toc_level(id).is_some() || style.name.starts_with("index ") => {
                        false
                    }
                    _ => doc
                        .styles
                        .heading_level(id)
                        .is_none_or(|level| level <= last_heading),
                }
            };
            visible.then_some(id)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_current_style_stays_in_this_document_without_changing_the_policy() {
        let doc = Document::new();
        let toc = doc.styles.find("toc 9").unwrap();
        assert!(!paragraph_styles(&doc, None).contains(&toc));
        assert!(paragraph_styles(&doc, Some(toc)).contains(&toc));
        assert!(!paragraph_styles(&doc, Some(toc)).contains(&doc.styles.find("toc 8").unwrap()));
    }
}
