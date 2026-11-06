//! Style ids for .docx. The original's automatic style names ("heading 1",
//! "header", "toc 1") are the same names Word still uses for its built-in
//! styles, so they map one to one.

use quill_core::Document;

/// The .docx style id Word uses for a built-in style name.
pub fn builtin_id(name: &str) -> Option<String> {
    let n = name.to_ascii_lowercase();
    let simple = match n.as_str() {
        "normal" => Some("Normal"),
        "header" => Some("Header"),
        "footer" => Some("Footer"),
        "footnote text" => Some("FootnoteText"),
        "footnote reference" => Some("FootnoteReference"),
        "page number" => Some("PageNumber"),
        "line number" => Some("LineNumber"),
        "postscript" => Some("PostScript"),
        _ => None,
    };
    if let Some(s) = simple {
        return Some(s.to_string());
    }
    for (prefix, id) in [
        ("heading ", "Heading"),
        ("index ", "Index"),
        ("toc ", "TOC"),
    ] {
        if let Some(num) = n.strip_prefix(prefix)
            && num.parse::<u8>().is_ok()
        {
            return Some(format!("{id}{num}"));
        }
    }
    None
}

/// A .docx style id for every style in the document, indexed by style id.
pub fn style_ids(doc: &Document) -> Vec<String> {
    let mut ids: Vec<String> = Vec::with_capacity(doc.styles.styles.len());
    for s in &doc.styles.styles {
        let base = builtin_id(&s.name).unwrap_or_else(|| {
            let cleaned: String = s
                .name
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect();
            if cleaned.is_empty() {
                "Style".to_string()
            } else {
                cleaned
            }
        });
        let mut id = base.clone();
        let mut n = 2;
        while ids.contains(&id) {
            id = format!("{base}{n}");
            n += 1;
        }
        ids.push(id);
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_names() {
        assert_eq!(builtin_id("heading 1").as_deref(), Some("Heading1"));
        assert_eq!(builtin_id("Normal").as_deref(), Some("Normal"));
        assert_eq!(builtin_id("toc 9").as_deref(), Some("TOC9"));
        assert_eq!(builtin_id("My Style"), None);
    }

    #[test]
    fn ids_are_unique() {
        let mut doc = Document::new();
        let pap = doc.styles.get(0).pap.clone();
        let chp = doc.styles.get(0).chp;
        doc.styles.add("Body Text", 0, pap.clone(), chp);
        doc.styles.add("Body-Text", 0, pap, chp);
        let ids = style_ids(&doc);
        assert_eq!(ids[0], "Normal");
        let n = ids.len();
        assert_eq!(ids[n - 2], "BodyText");
        assert_eq!(ids[n - 1], "BodyText2");
    }
}
