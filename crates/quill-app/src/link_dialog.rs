//! Edit > Hyperlink: the text to link, where it goes, whether it opens
//! in a new window, and its colors. The same dialog edits a link the
//! cursor is in, and can remove it.

use crate::dialog::Dialog;
use crate::docview::{DocView, LinkTarget};
use quill_core::Link;
use quill_core::link::{BROWSER_BLUE, BROWSER_PURPLE};
use quill_core::props::{Rgb, WORD_COLORS};

/// What the dialog started from, and the colors its menus offer.
#[derive(Clone, Debug)]
pub struct LinkState {
    pub target: LinkTarget,
    colors: Vec<(String, Rgb)>,
    /// The address the dialog opened with.
    url: String,
}

/// A browser's colors, then the original's eight, then any color the link has
/// that is none of those.
fn colors(have: &[Rgb]) -> Vec<(String, Rgb)> {
    let mut out: Vec<(String, Rgb)> = vec![
        ("Link Blue".into(), BROWSER_BLUE),
        ("Visited Purple".into(), BROWSER_PURPLE),
    ];
    out.extend(WORD_COLORS.iter().map(|(n, c)| (n.to_string(), *c)));
    for &c in have {
        if !out.iter().any(|(_, x)| *x == c) {
            out.push((format!("#{:06X}", c.0 & 0xFF_FFFF), c));
        }
    }
    out
}

pub fn dialog(view: &DocView) -> (Dialog, LinkState) {
    let target = view.link_target();
    let existing = if target.link != 0 {
        target.link
    } else {
        target.within
    };
    let link = quill_core::link::get(existing)
        .map(|l| (*l).clone())
        .unwrap_or_else(|| Link::new(""));
    let colors = colors(&[target.color, link.hover, link.visited]);
    let names: Vec<&str> = colors.iter().map(|(n, _)| n.as_str()).collect();
    let index = |c: Rgb| colors.iter().position(|(_, x)| *x == c).unwrap_or(0);

    let title = if target.link == 0 {
        "Create Hyperlink"
    } else {
        "Edit Hyperlink"
    };
    // A selection that holds a link starts with what to do about it.
    let top = if target.within != 0 { 66.0 } else { 0.0 };
    let mut d = Dialog::new(title, 412.0, 206.0 + top);
    if target.within != 0 {
        d.text("The selection already has a link:", 12.0, 12.0);
        d.radio(
            "stretch",
            "Stretch It, Keeping Its Address",
            24.0,
            30.0,
            2,
            true,
        );
        d.radio("fresh", "Use a New Address Instead", 24.0, 48.0, 2, false);
    }
    d.text("Text to Link:", 12.0, top + 12.0);
    d.field("text", &target.text, 110.0, top + 9.0, 192.0);
    d.text("Address:", 12.0, top + 40.0);
    d.field("url", &link.url, 110.0, top + 37.0, 192.0);
    d.text("Open In:", 12.0, top + 68.0);
    d.radio(
        "same",
        "Current Window",
        110.0,
        top + 68.0,
        1,
        !link.new_window,
    );
    d.radio("new", "New Window", 110.0, top + 86.0, 1, link.new_window);
    d.group("Colors", 10.0, top + 112.0, 294.0, 86.0);
    d.text("Link:", 20.0, top + 132.0);
    d.popup(
        "color",
        &names,
        index(target.color),
        100.0,
        top + 129.0,
        192.0,
    );
    d.text("Hover:", 20.0, top + 154.0);
    d.popup(
        "hover",
        &names,
        index(link.hover),
        100.0,
        top + 151.0,
        192.0,
    );
    d.text("Visited:", 20.0, top + 176.0);
    d.popup(
        "visited",
        &names,
        index(link.visited),
        100.0,
        top + 173.0,
        192.0,
    );
    let editing = target.link != 0;
    d.button(
        "ok",
        if editing { "Update" } else { "Insert" },
        318.0,
        8.0,
        82.0,
        true,
    );
    d.button("cancel", "Cancel", 318.0, 36.0, 82.0, false);
    d.button("remove", "Remove", 318.0, 64.0, 82.0, false);
    d.enable("remove", editing);
    d.focus(if target.text.is_empty() || target.within != 0 {
        "text"
    } else {
        "url"
    });
    let url = link.url;
    let st = LinkState {
        target,
        colors,
        url,
    };
    changed(&mut d, &st);
    (d, st)
}

/// The text, link and color the dialog describes, or None while either
/// field is empty.
pub fn read(d: &Dialog, st: &LinkState) -> Option<(String, Link, Rgb)> {
    let text = d.field_text("text");
    let url = d.field_text("url").trim().to_string();
    if text.is_empty() || url.is_empty() {
        return None;
    }
    let color = |id| {
        st.colors
            .get(d.selected(id))
            .map_or(BROWSER_BLUE, |(_, c)| *c)
    };
    let link = Link {
        url,
        new_window: d.radio_on(1) == "new",
        hover: color("hover"),
        visited: color("visited"),
    };
    Some((text, link, color("color")))
}

/// Insert stays dimmed until there is both text and an address. Stretching
/// a link keeps its address; asking for a new one empties the field once.
pub fn changed(d: &mut Dialog, st: &LinkState) {
    if st.target.within != 0 {
        let stretch = d.is_on("stretch");
        if stretch {
            d.set_field("url", &st.url);
        } else if d.field_text("url") == st.url {
            d.set_field("url", "");
        }
        d.enable("url", !stretch);
    }
    let ready = read(d, st).is_some();
    d.enable("ok", ready);
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::{Document, Position, Selection};

    #[test]
    fn selected_text_fills_the_dialog_and_a_link_fills_the_rest() {
        let mut view = DocView::new(Document::new(), "Link".into());
        view.type_text("Visit the site today");
        view.sel = Selection::new(Position::new(0, 10), Position::new(0, 14));
        let (mut d, st) = dialog(&view);
        assert_eq!(d.title, "Create Hyperlink");
        assert_eq!(d.field_text("text"), "site");
        assert_eq!(d.field_text("url"), "");
        assert!(read(&d, &st).is_none(), "no address yet");
        d.set_field("url", "https://example.com");
        d.set_on("same", false);
        d.set_on("new", true);
        let (text, link, color) = read(&d, &st).unwrap();
        assert_eq!(text, "site");
        assert!(link.new_window);
        assert_eq!(color, BROWSER_BLUE);
        assert_eq!(link.visited, BROWSER_PURPLE);
        view.apply_link(&st.target, &text, link, color);

        view.sel = Selection::caret(Position::new(0, 12));
        let (d, st) = dialog(&view);
        assert_eq!(d.field_text("text"), "site");
        assert_eq!(d.title, "Edit Hyperlink");
        assert_eq!(d.field_text("url"), "https://example.com");
        assert_eq!(d.radio_on(1), "new");
        assert_ne!(st.target.link, 0);
    }
}
