//! Edit > Hyperlink: the text to link, where it goes, whether it opens
//! in a new window, and its colors. The same dialog edits a link the
//! cursor is in, and can remove it.

use crate::dialog::Dialog;
use crate::docview::{DocView, LinkTarget};
use quill_core::Link;
use quill_core::link::BROWSER_BLUE;
use quill_core::props::Rgb;

/// What the dialog started from.
#[derive(Clone, Debug)]
pub struct LinkState {
    pub target: LinkTarget,
    /// The address the dialog opened with.
    url: String,
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
    let title = if target.link == 0 {
        "Create Hyperlink"
    } else {
        "Edit Hyperlink"
    };
    // A selection that holds a link starts with what to do about it.
    let top = if target.within != 0 { 66.0 } else { 0.0 };
    let mut d = Dialog::new(title, 412.0, 242.0 + top);
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
    // Each color is a box that opens the palette, with its HTML code beside.
    for (id, hex, label, y, c) in [
        ("color", "color_hex", "Link:", 129.0, target.color),
        ("hover", "hover_hex", "Hover:", 151.0, link.hover),
        ("visited", "visited_hex", "Visited:", 173.0, link.visited),
    ] {
        d.text(label, 20.0, top + y + 3.0);
        d.swatch_button(id, 100.0, top + y + 1.0, 44.0, 20.0, c.0);
        d.text_as(hex, &crate::palette::hex(c), 152.0, top + y + 3.0);
    }
    let editing = target.link != 0;
    d.action_buttons(&[
        ("remove", "Remove", 82.0, false),
        ("cancel", "Cancel", 82.0, false),
        ("ok", if editing { "Update" } else { "Insert" }, 82.0, true),
    ]);
    d.enable("remove", editing);
    d.focus(if target.text.is_empty() || target.within != 0 {
        "text"
    } else {
        "url"
    });
    let url = link.url;
    let st = LinkState { target, url };
    changed(&mut d, &st);
    (d, st)
}

/// The text, link and color the dialog describes, or None while either
/// field is empty.
pub fn read(d: &Dialog, _st: &LinkState) -> Option<(String, Link, Rgb)> {
    let text = d.field_text("text");
    let url = d.field_text("url").trim().to_string();
    if text.is_empty() || url.is_empty() {
        return None;
    }
    let color = |id| d.swatch_rgb(id).map_or(BROWSER_BLUE, Rgb);
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
    use quill_core::link::BROWSER_PURPLE;
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
