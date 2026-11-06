//! Hyperlinks. A linked run's `CharProps::link` names an entry here.
//!
//! Character formats are small `Copy` values, so a run cannot hold the
//! address itself. Instead every link Quill sees is kept once in a table
//! shared by all open documents, and runs hold its number. Equal links
//! get the same number, so neighboring runs of one link still merge,
//! and text copied from one window to another keeps its links with no
//! renumbering. Entries are never removed; a link is a few dozen bytes.

use crate::props::Rgb;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

/// A browser's color for a link nobody has followed yet.
pub const BROWSER_BLUE: Rgb = Rgb(0x0000EE);
/// A browser's color for a link that has been followed.
pub const BROWSER_PURPLE: Rgb = Rgb(0x551A8B);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Link {
    /// Where the link goes: a web address, a mailto: address or a file.
    pub url: String,
    /// Open the target in a new window (HTML's target="_blank").
    pub new_window: bool,
    /// Color under the mouse pointer. Only HTML export uses it: Word,
    /// RTF and OpenDocument have nothing like it.
    pub hover: Rgb,
    /// Color once followed. Only HTML export uses it, for the same
    /// reason.
    pub visited: Rgb,
}

impl Link {
    /// A link to `url` with a browser's colors, opening in the same window.
    pub fn new(url: &str) -> Link {
        Link {
            url: url.to_string(),
            new_window: false,
            hover: BROWSER_BLUE,
            visited: BROWSER_PURPLE,
        }
    }
}

#[derive(Default)]
struct Table {
    links: Vec<Arc<Link>>,
    ids: HashMap<Arc<Link>, u32>,
}

fn table() -> &'static RwLock<Table> {
    static TABLE: OnceLock<RwLock<Table>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

/// The number for `link`, adding it to the table if it is new. Never 0,
/// which means "no link".
pub fn intern(link: Link) -> u32 {
    if let Some(&id) = table()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .ids
        .get(&link)
    {
        return id;
    }
    let mut t = table().write().unwrap_or_else(|e| e.into_inner());
    if let Some(&id) = t.ids.get(&link) {
        return id;
    }
    let link = Arc::new(link);
    t.links.push(link.clone());
    let id = t.links.len() as u32;
    t.ids.insert(link, id);
    id
}

/// The link numbered `id`, or None for 0 and numbers never handed out.
pub fn get(id: u32) -> Option<Arc<Link>> {
    let i = (id as usize).checked_sub(1)?;
    table()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .links
        .get(i)
        .cloned()
}

/// Just the address of link `id`, or "" when there is none.
pub fn url(id: u32) -> String {
    get(id).map(|l| l.url.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_links_share_a_number() {
        let a = intern(Link::new("https://example.com/a"));
        let b = intern(Link::new("https://example.com/a"));
        let c = intern(Link {
            new_window: true,
            ..Link::new("https://example.com/a")
        });
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, 0);
        assert_eq!(get(a).unwrap().url, "https://example.com/a");
        assert!(get(c).unwrap().new_window);
        assert!(get(0).is_none());
        assert!(get(u32::MAX).is_none());
    }
}
