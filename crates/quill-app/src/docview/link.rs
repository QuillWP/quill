//! Hyperlinks: finding the link at the cursor, making and changing one
//! (Edit > Hyperlink), taking links off, and following one.

use super::*;
use quill_core::Link;
use quill_core::props::Rgb;

/// The text the Hyperlink dialog works on, and the link it has now.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkTarget {
    pub start: Position,
    pub end: Position,
    /// The text as it is, for the dialog's "Text to link" field.
    pub text: String,
    /// The link the text already is (0 for none), whose settings the
    /// dialog starts from.
    pub link: u32,
    /// The first link in a selection that runs past it (0 for none), which
    /// the dialog offers to stretch over the whole selection.
    pub within: u32,
    /// The color of the linked text.
    pub color: Rgb,
}

impl DocView {
    /// The whole link around the main text position `at`, if there is one.
    fn link_around(&self, at: Position) -> Option<(Position, Position, u32)> {
        let (s, e, link) = self.doc.main.paras[at.para].link_range(at.offset)?;
        Some((Position::new(at.para, s), Position::new(at.para, e), link))
    }

    /// What the Hyperlink dialog edits: with nothing selected, the link the
    /// cursor is in (or an empty spot for a new one); otherwise the
    /// selection, grown to the whole link when it lies inside one.
    pub fn link_target(&self) -> LinkTarget {
        let (s, e) = self.sel.range();
        let around = self
            .link_around(s)
            .filter(|&(ls, le, _)| ls <= s && e <= le);
        let (start, end, link) = match around {
            Some(found) if s == e || found.0 <= s => found,
            _ => (s, e, 0),
        };
        let text = if start == end {
            String::new()
        } else {
            self.doc.copy(MAIN, start, end).plain_text()
        };
        let within = if link == 0 && s != e {
            self.first_link_in(s, e)
        } else {
            None
        };
        let color = match (link, within) {
            (0, None) => self
                .doc
                .styles
                .find_character("Hyperlink")
                .and_then(|id| self.doc.styles.character(id))
                .map_or(quill_core::link::BROWSER_BLUE, |s| {
                    s.format
                        .resolved(self.doc.styles.get(self.current_pap().style).chp)
                        .color
                }),
            (0, Some((_, color))) => color,
            _ => {
                self.doc.main.paras[start.para]
                    .chp_of_char(start.offset)
                    .color
            }
        };
        LinkTarget {
            start,
            end,
            text,
            link,
            within: within.map_or(0, |(id, _)| id),
            color,
        }
    }

    /// The first link in `s..e`, and its color.
    fn first_link_in(&self, s: Position, e: Position) -> Option<(u32, Rgb)> {
        let paras = &self.doc.main.paras;
        (s.para..=e.para).find_map(|i| {
            let p = &paras[i];
            let a = if i == s.para { s.offset } else { 0 };
            let b = if i == e.para { e.offset } else { p.len() };
            p.run_bounds()
                .find(|(rs, re, c)| c.link != 0 && *rs < b && *re > a)
                .map(|(_, _, c)| (c.link, c.color))
        })
    }

    /// Make `target`'s text into `link`, shown in `color` and underlined.
    /// Text the dialog changed replaces the old; text it left alone keeps
    /// its own formatting apart from the link's color and underline.
    pub fn apply_link(&mut self, target: &LinkTarget, text: &str, link: Link, color: Rgb) {
        let id = quill_core::link::intern(link);
        self.doc.begin("Hyperlink", self.sel);
        let (start, mut end) = (target.start, target.end);
        if text != target.text || start == end {
            let at = if start == end {
                start
            } else {
                self.delete_range(start, end)
            };
            let chp = self.doc.main.paras[at.para].chp_at(at.offset);
            // Line ends in the field become line breaks, not paragraphs.
            let text: String = text
                .chars()
                .map(|c| if c == '\n' { chars::LINE_BREAK } else { c })
                .filter(|c| !c.is_control() || *c == chars::LINE_BREAK || *c == '\t')
                .collect();
            end = self.doc.insert_text(MAIN, at, &text, chp);
        }
        self.doc.set_link(MAIN, start, end, id, color);
        self.typing_chp = None;
        self.finish_edit(Selection::caret(end));
    }

    /// Take the link off `start..end`: its text goes back to the
    /// paragraph's color with no underline.
    pub fn remove_link_from(&mut self, start: Position, end: Position) {
        self.doc.begin("Remove Link", self.sel);
        self.doc.set_link(MAIN, start, end, 0, Rgb::BLACK);
        self.typing_chp = None;
        let sel = self.sel;
        self.finish_edit(sel);
    }

    /// Remove Link: the link the cursor is in, or every link the selection
    /// touches, each one whole.
    pub fn remove_links(&mut self) {
        let (mut s, mut e) = self.sel.range();
        if let Some((ls, _, _)) = self.link_around(s) {
            s = s.min(ls);
        }
        if let Some((_, le, _)) = self.link_around(e) {
            e = e.max(le);
        }
        if s != e {
            self.remove_link_from(s, e);
        }
    }

    /// Whether the cursor is in a link or the selection holds one.
    pub fn has_link(&self) -> bool {
        let (s, e) = self.sel.range();
        if self.link_around(s).is_some() {
            return true;
        }
        let paras = &self.doc.main.paras;
        (s.para..=e.para).any(|i| {
            let p = &paras[i];
            let a = if i == s.para { s.offset } else { 0 };
            let b = if i == e.para { e.offset } else { p.len() };
            p.run_bounds()
                .any(|(rs, re, c)| c.link != 0 && rs < b && re > a)
        })
    }

    /// The address of the link a click at `at` landed on: a click falls
    /// between two characters, so either one counts.
    pub fn link_url_near(&self, at: Position) -> Option<String> {
        let before = Position::new(at.para, at.offset.saturating_sub(1));
        self.link_url_at(at)
            .or_else(|| self.link_url_at(before).filter(|_| at.offset > 0))
    }

    /// Open `url`, saying so in the status line if Quill won't.
    pub fn open_link(&mut self, url: &str) {
        if !follow(url) {
            self.status = Some(format!("Quill does not open links like {url}"));
        }
    }

    /// The address of the link at main text position `at`: the character
    /// there, not the one before it, so a click just past a link misses.
    pub fn link_url_at(&self, at: Position) -> Option<String> {
        let p = self.doc.main.paras.get(at.para)?;
        if at.offset >= p.len() {
            return None;
        }
        let link = p.chp_of_char(at.offset).link;
        quill_core::link::get(link).map(|l| l.url.clone())
    }
}

/// Open `url` in the program the system uses for it: the browser for web
/// addresses, Mail for mailto: and so on. Only schemes a link should open
/// are passed on, so a document cannot run a program by linking to it.
pub fn follow(url: &str) -> bool {
    let url = url.trim();
    let scheme = url
        .split_once(':')
        .map(|(s, _)| s.to_ascii_lowercase())
        .unwrap_or_default();
    let url = if scheme.is_empty() && url.contains('.') && !url.contains(' ') {
        // "example.com" with no scheme, as people often type.
        format!("https://{url}")
    } else if ["http", "https", "mailto", "ftp", "tel"].contains(&scheme.as_str()) {
        url.to_string()
    } else {
        return false;
    };
    #[cfg(target_os = "macos")]
    let opener = std::process::Command::new("/usr/bin/open")
        .arg(&url)
        .spawn();
    #[cfg(target_os = "windows")]
    let opener = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", &url])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let opener = std::process::Command::new("xdg-open").arg(&url).spawn();
    opener.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(text: &str) -> DocView {
        DocView::new(quill_core::text::from_plain_text(text), "Links".into())
    }

    #[test]
    fn a_selection_past_a_link_can_stretch_it_or_replace_it() {
        let old = Link::new("https://old.example");
        let mut v = view("see the manual first");
        let manual = LinkTarget {
            start: Position::new(0, 8),
            end: Position::new(0, 14),
            text: "manual".into(),
            link: 0,
            within: 0,
            color: Rgb(0xAA0000),
        };
        v.apply_link(&manual, "manual", old, Rgb(0xAA0000));
        let id = v.doc.main.paras[0].chp_of_char(8).link;
        v.sel = Selection::new(Position::new(0, 4), Position::new(0, 20));
        let t = v.link_target();
        assert_eq!((t.link, t.within), (0, id));
        assert_eq!(t.color, Rgb(0xAA0000));

        // Stretched: one link over all of it, at the same address.
        let (mut d, st) = crate::link_dialog::dialog(&v);
        assert_eq!(d.field_text("url"), "https://old.example");
        assert!(!d.is_enabled("url"));
        let (text, link, color) = crate::link_dialog::read(&d, &st).unwrap();
        v.apply_link(&t, &text, link, color);
        let p = &v.doc.main.paras[0];
        assert_eq!(p.link_range(5), Some((4, 20, id)));
        assert_eq!(p.chp_of_char(16).color, Rgb(0xAA0000));
        assert_eq!(p.chp_of_char(3).link, 0);
        let mut clip = Clipboard::without_system();
        v.exec(Cmd::Undo, false, &mut clip);

        // A new address: the old link goes and one new link covers it all.
        v.sel = Selection::new(Position::new(0, 4), Position::new(0, 20));
        let t = v.link_target();
        d.set_on("stretch", false);
        d.set_on("fresh", true);
        crate::link_dialog::changed(&mut d, &st);
        assert_eq!(d.field_text("url"), "");
        assert!(d.is_enabled("url"));
        assert!(crate::link_dialog::read(&d, &st).is_none());
        d.set_field("url", "https://new.example");
        let (text, link, color) = crate::link_dialog::read(&d, &st).unwrap();
        v.apply_link(&t, &text, link, color);
        let p = &v.doc.main.paras[0];
        let (s, e, new) = p.link_range(5).unwrap();
        assert_eq!((s, e), (4, 20));
        assert_ne!(new, id);
        assert_eq!(quill_core::link::url(new), "https://new.example");
    }

    #[test]
    fn make_change_and_remove_a_link() {
        let mut v = view("Read the manual first");
        v.sel = Selection::new(Position::new(0, 9), Position::new(0, 15));
        let t = v.link_target();
        assert_eq!(t.text, "manual");
        assert_eq!(t.link, 0);
        v.apply_link(
            &t,
            "manual",
            Link::new("https://example.com/manual"),
            quill_core::link::BROWSER_BLUE,
        );
        let p = &v.doc.main.paras[0];
        assert_eq!(p.text, "Read the manual first");
        let c = p.chp_of_char(10);
        assert_eq!(quill_core::link::url(c.link), "https://example.com/manual");
        assert_eq!(c.underline, Underline::Single);
        assert_eq!(c.color, quill_core::link::BROWSER_BLUE);
        assert_eq!(p.chp_of_char(8).link, 0);
        assert_eq!(p.chp_of_char(15).link, 0);

        // The cursor anywhere in it finds the whole link again.
        v.sel = Selection::caret(Position::new(0, 12));
        let t = v.link_target();
        assert_eq!((t.start.offset, t.end.offset), (9, 15));
        assert_eq!(t.text, "manual");
        assert_ne!(t.link, 0);
        assert!(v.has_link());

        // New text replaces the old, still linked.
        v.apply_link(
            &t,
            "guide",
            Link::new("https://example.com/guide"),
            quill_core::link::BROWSER_BLUE,
        );
        let p = &v.doc.main.paras[0];
        assert_eq!(p.text, "Read the guide first");
        assert_eq!(
            quill_core::link::url(p.chp_of_char(9).link),
            "https://example.com/guide"
        );
        assert_eq!(
            v.link_url_at(Position::new(0, 9)).as_deref(),
            Some("https://example.com/guide")
        );
        assert_eq!(v.link_url_at(Position::new(0, 14)), None);

        v.sel = Selection::caret(Position::new(0, 11));
        v.remove_links();
        let p = &v.doc.main.paras[0];
        assert!(p.runs.iter().all(|r| r.chp.link == 0));
        assert_eq!(p.chp_of_char(10).underline, Underline::None);
        assert!(!v.has_link());
        assert!(v.doc.undo().is_some());
        assert_ne!(v.doc.main.paras[0].chp_of_char(10).link, 0);
    }

    #[test]
    fn a_new_link_at_the_cursor_inserts_its_text() {
        let mut v = view("See ");
        v.sel = Selection::caret(Position::new(0, 4));
        let t = v.link_target();
        assert_eq!(t.text, "");
        v.apply_link(
            &t,
            "this page",
            Link::new("https://example.com/"),
            quill_core::link::BROWSER_BLUE,
        );
        let p = &v.doc.main.paras[0];
        assert_eq!(p.text, "See this page");
        assert_ne!(p.chp_of_char(4).link, 0);
        assert_eq!(v.sel, Selection::caret(Position::new(0, 13)));
        // Typing goes on after the link, not in it.
        v.type_text(" now");
        let p = &v.doc.main.paras[0];
        assert_eq!(p.chp_of_char(14).link, 0);
        assert_eq!(p.chp_of_char(14).underline, Underline::None);
    }

    #[test]
    fn typed_addresses_are_recognised_without_trailing_punctuation() {
        use super::super::editing::typed_address;
        let link = |w: &str| typed_address(w).map(|(range, url)| (w[range].to_string(), url));
        assert_eq!(
            link("https://example.com/a?b=1."),
            Some((
                "https://example.com/a?b=1".into(),
                "https://example.com/a?b=1".into()
            ))
        );
        assert_eq!(
            link("(http://example.com)."),
            Some(("http://example.com".into(), "http://example.com".into()))
        );
        assert_eq!(
            link("http://en.wikipedia.org/wiki/Quill_(app))"),
            Some((
                "http://en.wikipedia.org/wiki/Quill_(app)".into(),
                "http://en.wikipedia.org/wiki/Quill_(app)".into()
            ))
        );
        assert_eq!(
            link("www.example.com,"),
            Some(("www.example.com".into(), "https://www.example.com".into()))
        );
        assert_eq!(
            link("MAILTO:ada@example.com"),
            Some((
                "MAILTO:ada@example.com".into(),
                "MAILTO:ada@example.com".into()
            ))
        );
        assert_eq!(link("mailto:nobody"), None);
        assert_eq!(
            typed_address("lucas@example.com.").map(|a| a.1).as_deref(),
            Some("mailto:lucas@example.com")
        );
        assert_eq!(typed_address("a@b"), None);
        assert_eq!(typed_address("@example.com"), None);
        assert_eq!(typed_address("one@two@three.com"), None);
        assert_eq!(link("http://"), None);
        assert_eq!(link("www."), None);
        assert_eq!(link("example.com"), None);
        assert_eq!(link("https://a\u{FFFC}b"), None);
    }

    #[test]
    fn a_typed_address_becomes_a_link_that_undo_takes_off() {
        let mut clip = Clipboard::without_system();
        let mut v = view("");
        v.type_text("See https://example.com.");
        v.type_text(" ");
        let p = &v.doc.main.paras[0];
        assert_eq!(p.text, "See https://example.com. ");
        let c = p.chp_of_char(4);
        assert_eq!(quill_core::link::url(c.link), "https://example.com");
        assert_eq!(c.underline, Underline::Single);
        assert_eq!(p.chp_of_char(3).link, 0);
        assert_eq!(p.chp_of_char(23).link, 0, "the full stop stays out");
        assert_eq!(v.sel, Selection::caret(Position::new(0, 25)));
        // Typing on is plain text.
        v.type_text("ok");
        assert_eq!(v.doc.main.paras[0].chp_of_char(25).link, 0);
        // One Undo takes "ok" back, the next the link, keeping the text.
        v.exec(Cmd::Undo, false, &mut clip);
        v.exec(Cmd::Undo, false, &mut clip);
        let p = &v.doc.main.paras[0];
        assert_eq!(p.text, "See https://example.com. ");
        assert_eq!(p.chp_of_char(4).link, 0);
        assert_eq!(p.chp_of_char(4).underline, Underline::None);
    }

    #[test]
    fn return_and_tab_also_make_links_and_existing_links_are_left_alone() {
        let mut clip = Clipboard::without_system();
        let mut v = view("");
        v.type_text("www.example.com");
        v.exec(Cmd::NewParagraph, false, &mut clip);
        let c = v.doc.main.paras[0].chp_of_char(0);
        assert_eq!(quill_core::link::url(c.link), "https://www.example.com");
        v.type_text("mailto:ada@example.com");
        v.exec(Cmd::Tab, false, &mut clip);
        let c = v.doc.main.paras[1].chp_of_char(0);
        assert_eq!(quill_core::link::url(c.link), "mailto:ada@example.com");

        // A link made by hand keeps its own address.
        let mut v = view("https://shown.example");
        v.sel = Selection::new(Position::new(0, 0), Position::new(0, 21));
        let t = v.link_target();
        v.apply_link(
            &t,
            "https://shown.example",
            Link::new("https://real.example"),
            quill_core::link::BROWSER_BLUE,
        );
        v.type_text(" ");
        let c = v.doc.main.paras[0].chp_of_char(0);
        assert_eq!(quill_core::link::url(c.link), "https://real.example");
    }

    #[test]
    fn a_click_on_either_side_of_a_character_finds_its_link() {
        let mut v = view("See this page now");
        v.sel = Selection::new(Position::new(0, 4), Position::new(0, 8));
        let t = v.link_target();
        v.apply_link(
            &t,
            "this",
            Link::new("https://example.com"),
            quill_core::link::BROWSER_BLUE,
        );
        let at = |offset| v.link_url_near(Position::new(0, offset));
        assert_eq!(at(4).as_deref(), Some("https://example.com"));
        assert_eq!(at(8).as_deref(), Some("https://example.com"));
        assert_eq!(at(0), None);
        assert_eq!(at(10), None);
    }

    #[test]
    fn typed_addresses_stay_text_when_the_setting_is_off() {
        crate::settings::update(|s| s.auto_links = false);
        let mut v = view("");
        v.type_text("https://example.com");
        v.type_text(" ");
        crate::settings::update(|s| s.auto_links = true);
        assert_eq!(v.doc.main.paras[0].chp_of_char(0).link, 0);
    }

    #[test]
    fn only_web_and_mail_addresses_are_followed_blindly() {
        assert!(!follow("file:///Applications/Calculator.app"));
        assert!(!follow("javascript:alert(1)"));
        assert!(!follow("not a link"));
    }
}
