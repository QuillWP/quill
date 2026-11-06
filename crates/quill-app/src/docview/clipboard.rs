//! System HTML/RTF/plain-text exchange and Quill's exact selection cache.

use super::*;

#[path = "clipboard_rich.rs"]
mod rich;
use rich::RichContent;

/// Most clipboard text, and HTML, Quill will read: the text limit is the
/// document's character limit, and the HTML limit is the HTML reader's.
const MAX_CLIP_TEXT: usize = quill_core::limits::MAX_CHARACTERS;
const MAX_CLIP_HTML: usize = 8 << 20;

/// What a paste brings in.
pub enum Clip {
    /// Copied inside Quill, with formatting.
    Rich(RichContent),
    /// Text from another application.
    Plain(String),
    /// A picture another application copied (PNG, JPEG, TIFF and so on),
    /// with no text beside it: a screenshot, or Copy Image in a browser.
    Picture(Vec<u8>),
}

/// Whether a copied picture is pasted rather than the text copied with it.
/// Programs that copy formatted text often add a picture of it too, so the
/// text wins, unless it is only the picture's web address.
fn picture_wins(text: Option<&str>) -> bool {
    let text = text.map_or("", str::trim);
    text.is_empty()
        || (!text.contains(char::is_whitespace)
            && ["http://", "https://", "file://"]
                .iter()
                .any(|p| text.starts_with(p)))
}

/// Standard clipboard representations plus an exact in-process selection.
pub struct Clipboard {
    system: Option<arboard::Clipboard>,
    rich: Option<Cached>,
    local_text: Option<String>,
}

struct Cached {
    text: String,
    html: String,
    sequence: Option<u64>,
    content: RichContent,
}

impl Cached {
    fn owns(&self, text: Option<&str>, html: Option<&str>, sequence: Option<u64>) -> bool {
        text == Some(self.text.as_str())
            && match (self.sequence, sequence) {
                (Some(old), Some(now)) => old == now,
                _ => html == Some(self.html.as_str()),
            }
    }
}

impl Clipboard {
    pub fn new() -> Self {
        Clipboard {
            // Editing tests must not read or replace the person's clipboard.
            system: if cfg!(test) {
                None
            } else {
                arboard::Clipboard::new().ok()
            },
            rich: None,
            local_text: None,
        }
    }

    /// A clipboard that never touches the system's, so tests running side
    /// by side do not copy over one another.
    #[cfg(test)]
    pub fn without_system() -> Self {
        Clipboard {
            system: None,
            rich: None,
            local_text: None,
        }
    }

    /// What Paste would paste, as a fragment (plain text from another
    /// application becomes paragraphs in the default format).
    pub fn fragment(&mut self) -> Option<Fragment> {
        match self.get()? {
            Clip::Rich(f) => Some(f.fragment),
            Clip::Picture(_) => None,
            Clip::Plain(t) => {
                let t = t.replace("\r\n", "\n").replace('\r', "\n");
                let mut lines: Vec<&str> = t.split('\n').collect();
                let ends_with_mark = lines.len() > 1 && lines.last() == Some(&"");
                if ends_with_mark {
                    lines.pop();
                }
                let paras = lines
                    .iter()
                    .map(|l| Paragraph::with_text(l, CharProps::default(), ParaProps::default()))
                    .collect();
                Some(Fragment {
                    paras,
                    ends_with_mark,
                    objects: Vec::new(),
                    notes: Vec::new(),
                })
            }
        }
    }

    /// The forms Paste Special can offer for what the Clipboard holds.
    pub fn forms(&mut self) -> Vec<crate::paste_special::Form> {
        use crate::paste_special::Form;
        let mut out = match self.get() {
            Some(Clip::Rich(_)) => vec![Form::Formatted, Form::Unformatted],
            Some(Clip::Plain(t)) if !t.is_empty() => vec![Form::Unformatted],
            _ => Vec::new(),
        };
        if self.system.is_some()
            && crate::paste_special::clipboard_image()
                .and_then(quill_formats::images::picture)
                .is_some()
        {
            out.push(Form::Picture);
        }
        out
    }

    pub fn put_text(&mut self, text: &str) {
        if let Some(system) = &mut self.system {
            let _ = system.set_text(text.to_string());
        }
        self.rich = None;
        self.local_text = Some(text.to_string());
    }

    pub fn put(&mut self, source: &Document, frag: Fragment) {
        // What is copied is the text with its tracked changes accepted:
        // other programs can't show them, and Quill's own paste would drop
        // them anyway.
        let frag = Fragment {
            paras: quill_core::tracking::accepted(&frag.paras),
            ..frag
        };
        let mut text = frag.plain_text();
        if cfg!(windows) {
            text = text.replace('\n', "\r\n");
        }
        // Preferences > Include Formatted Text in Clipboard: off means
        // pasting gives plain text even within Quill.
        if !crate::settings::with(|s| s.formatted_clipboard) {
            self.put_text(&text);
            return;
        }
        let content = RichContent::new(source, frag);
        let doc = content.document();
        let mut html = quill_formats::html::write_clipboard(&doc);
        // An ownership marker distinguishes a new external copy containing
        // exactly the same visible text on platforms without a sequence API.
        static NUMBER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        use std::sync::atomic::Ordering;
        html.push_str(&format!(
            "<!--quill-{}-{}-->",
            std::process::id(),
            NUMBER.fetch_add(1, Ordering::Relaxed)
        ));
        if let Some(s) = &mut self.system {
            let published = s.set_html(html.clone(), Some(text.clone())).is_ok()
                || s.set_text(text.clone()).is_ok();
            if published {
                let rtf =
                    quill_formats::rtf::write_clipboard(&doc, content.fragment.ends_with_mark);
                let _ = crate::system_clipboard::put_rtf(&rtf);
            }
        }
        self.rich = Some(Cached {
            text,
            html,
            sequence: self
                .system
                .as_ref()
                .and_then(|_| crate::system_clipboard::sequence()),
            content,
        });
    }

    /// Our own fragment if the system clipboard still holds what we put
    /// there, otherwise whatever text another application copied.
    pub fn get(&mut self) -> Option<Clip> {
        if self.system.is_none() {
            return self
                .rich
                .as_ref()
                .map(|c| Clip::Rich(c.content.clone()))
                .or_else(|| self.local_text.clone().map(Clip::Plain));
        }
        // The clipboard provider has already built these strings, so the
        // limits stop only what Quill does with them (parsing, pasting).
        let text = self
            .system
            .as_mut()
            .and_then(|s| s.get_text().ok())
            .filter(|t| t.len() <= MAX_CLIP_TEXT);
        let html = self
            .system
            .as_mut()
            .and_then(|s| s.get().html().ok())
            .filter(|h| h.len() <= MAX_CLIP_HTML);
        let sequence = crate::system_clipboard::sequence();
        if let Some(cached) = &self.rich
            && cached.owns(text.as_deref(), html.as_deref(), sequence)
        {
            return Some(Clip::Rich(cached.content.clone()));
        }
        // Another application's formatting, with its measurements kept
        // within range like a file's.
        if let Some(bytes) = crate::system_clipboard::rtf()
            && let Ok(mut doc) = quill_formats::rtf::read(&bytes)
        {
            doc.keep_in_bounds();
            crate::font_names::use_family_names(&mut doc);
            return Some(Clip::Rich(RichContent::from_document(doc, text.as_deref())));
        }
        if let Some(html) = html
            && let Ok(mut doc) = quill_formats::html::read_clipboard(&html)
            && doc.main.paras.iter().any(|p| !p.text.is_empty())
        {
            doc.keep_in_bounds();
            crate::font_names::use_family_names(&mut doc);
            return Some(Clip::Rich(RichContent::from_document(doc, text.as_deref())));
        }
        if picture_wins(text.as_deref())
            && let Some(bytes) = crate::paste_special::clipboard_image()
        {
            return Some(Clip::Picture(bytes));
        }
        text.map(Clip::Plain)
    }

    pub fn plain_text(&mut self) -> Option<String> {
        self.system
            .as_mut()
            .and_then(|s| s.get_text().ok())
            .filter(|t| t.len() <= MAX_CLIP_TEXT)
            .or_else(|| {
                self.get().map(|c| match c {
                    Clip::Plain(s) => s,
                    Clip::Rich(r) => r.fragment.plain_text(),
                    Clip::Picture(_) => String::new(),
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_clipboard_keeps_rich_content_without_system_ownership() {
        let mut clipboard = Clipboard::without_system();
        let doc = quill_core::text::from_plain_text("private copy");
        let fragment = Fragment {
            paras: doc.main.paras.clone(),
            ..Fragment::default()
        };
        clipboard.put(&doc, fragment);
        assert!(clipboard.rich.as_ref().unwrap().sequence.is_none());
        assert_eq!(clipboard.plain_text().as_deref(), Some("private copy"));
        assert_eq!(
            clipboard.forms(),
            [
                crate::paste_special::Form::Formatted,
                crate::paste_special::Form::Unformatted
            ]
        );
        clipboard.put_text("plain copy");
        assert!(matches!(clipboard.get(), Some(Clip::Plain(text)) if text == "plain copy"));
    }

    #[test]
    fn a_copied_picture_wins_only_over_no_text_or_its_address() {
        assert!(picture_wins(None));
        assert!(picture_wins(Some("  ")));
        assert!(picture_wins(Some("https://example.com/cat.png")));
        assert!(!picture_wins(Some("A paragraph copied from Word")));
        assert!(!picture_wins(Some("see https://example.com")));
    }

    #[test]
    fn equal_plain_text_does_not_reuse_stale_formatting_after_an_external_copy() {
        let doc = Document::new();
        let mut cache = Cached {
            text: "same text".into(),
            html: "<b>same text</b><!--unique-->".into(),
            sequence: Some(3),
            content: RichContent::new(&doc, Fragment::default()),
        };
        assert!(cache.owns(Some("same text"), None, Some(3)));
        assert!(!cache.owns(Some("same text"), None, Some(4)));
        assert!(!cache.owns(None, None, Some(3)));
        cache.sequence = None;
        assert!(cache.owns(Some("same text"), Some(&cache.html), None));
        assert!(!cache.owns(Some("same text"), Some("<i>same text</i>"), None));
    }
}
