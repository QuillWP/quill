//! Hyperlinks. A linked run's `CharProps::link` names an entry here.
//!
//! Character formats are small `Copy` values, so a run cannot hold the
//! address itself. Instead every link Quill sees is kept once in a table
//! shared by all open documents, and runs hold its number. Equal links
//! get the same number, so neighboring runs of one link still merge,
//! and text copied from one window to another keeps its links with no
//! renumbering. Entries are never removed, so the table is bounded
//! instead: see `MAX_URL_BYTES`, `MAX_LINKS` and `MAX_TABLE_BYTES`.

use crate::props::Rgb;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
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

/// The longest address kept. Browsers stop near 2,000 characters in
/// practice; this leaves room for long query strings and still refuses
/// absurd ones.
pub const MAX_URL_BYTES: usize = 16 * 1024;
/// The most distinct links one process remembers.
pub const MAX_LINKS: usize = 500_000;
/// The most memory the table's addresses may use in all.
pub const MAX_TABLE_BYTES: usize = 64 << 20;

#[derive(Default)]
struct Table {
    links: Vec<Arc<Link>>,
    ids: HashMap<Arc<Link>, u32>,
    /// Bytes held by the addresses so far.
    bytes: usize,
}

fn cost(link: &Link) -> usize {
    link.url.len() + std::mem::size_of::<Link>()
}

impl Table {
    /// Add `link` unless a limit would be passed; 0 (no link) if so.
    fn add(&mut self, link: Link, max_links: usize, max_bytes: usize) -> u32 {
        let cost = cost(&link);
        if link.url.len() > MAX_URL_BYTES
            || self.links.len() >= max_links
            || self.bytes.saturating_add(cost) > max_bytes
        {
            return 0;
        }
        self.bytes += cost;
        let link = Arc::new(link);
        self.links.push(link.clone());
        let id = self.links.len() as u32;
        self.ids.insert(link, id);
        id
    }

    /// Forget every entry after the first `len`.
    fn truncate(&mut self, len: usize) {
        if len >= self.links.len() {
            return;
        }
        for link in self.links.drain(len..) {
            self.ids.remove(&link);
            self.bytes -= cost(&link);
        }
    }
}

/// Calls to `intern` on every thread, and on this one. `rollback` compares
/// the two to know that no other thread can hold a number it would take
/// back.
static CALLS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static OWN_CALLS: Cell<u64> = const { Cell::new(0) };
    /// Links this thread was refused, because the address was too long or
    /// the table full.
    static DROPPED: Cell<usize> = const { Cell::new(0) };
}

fn table() -> &'static RwLock<Table> {
    static TABLE: OnceLock<RwLock<Table>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

/// The number for `link`, adding it to the table if it is new. 0 means
/// "no link": it comes back when the address or the table is past its
/// limit, so a document cannot grow the table without bound. Links
/// already in the table keep their numbers.
pub fn intern(link: Link) -> u32 {
    // Counted before the table is looked at, so a `rollback` that starts
    // after this either sees the call or runs before the lookup.
    CALLS.fetch_add(1, Ordering::SeqCst);
    OWN_CALLS.set(OWN_CALLS.get() + 1);
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
    let id = t.add(link, MAX_LINKS, MAX_TABLE_BYTES);
    if id == 0 {
        DROPPED.set(DROPPED.get() + 1);
    }
    id
}

/// How many links this thread has been refused so far. A reader compares
/// the count before and after a file to tell the person links were lost.
pub fn dropped() -> usize {
    DROPPED.get()
}

/// Where the table stood, so the links of a file Quill then refuses can be
/// taken out again (`rollback`).
#[derive(Clone, Copy, Debug)]
pub struct Mark {
    len: usize,
    calls: u64,
    own: u64,
}

pub fn mark() -> Mark {
    let t = table().read().unwrap_or_else(|e| e.into_inner());
    Mark {
        len: t.links.len(),
        calls: CALLS.load(Ordering::SeqCst),
        own: OWN_CALLS.get(),
    }
}

/// Take out the links added since `mark`, for a file that was read and
/// then refused, so it does not use up the table for the rest of the
/// session. Nothing is taken out if another thread interned a link since
/// then, since it could hold one of the numbers. True if it was done.
pub fn rollback(mark: Mark) -> bool {
    let mut t = table().write().unwrap_or_else(|e| e.into_inner());
    let others = CALLS.load(Ordering::SeqCst) - mark.calls - (OWN_CALLS.get() - mark.own);
    if others != 0 {
        return false;
    }
    t.truncate(mark.len);
    true
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

    #[test]
    fn long_addresses_and_a_full_table_give_no_link() {
        let long = Link::new(&"a".repeat(MAX_URL_BYTES + 1));
        assert_eq!(intern(long), 0);
        let edge = Link::new(&"a".repeat(MAX_URL_BYTES));
        assert_ne!(intern(edge), 0);
    }

    #[test]
    fn the_table_stops_growing_at_its_count_and_byte_limits() {
        let mut t = Table::default();
        assert_eq!(t.add(Link::new("https://a.example/1"), 2, usize::MAX), 1);
        assert_eq!(t.add(Link::new("https://a.example/2"), 2, usize::MAX), 2);
        assert_eq!(t.add(Link::new("https://a.example/3"), 2, usize::MAX), 0);
        // Numbers handed out earlier are still good.
        assert_eq!(t.links[0].url, "https://a.example/1");
        let one = std::mem::size_of::<Link>() + "https://b.example/1".len();
        let mut t = Table::default();
        assert_eq!(t.add(Link::new("https://b.example/1"), 10, one * 2), 1);
        assert_eq!(t.add(Link::new("https://b.example/2"), 10, one * 2), 2);
        assert_eq!(t.add(Link::new("https://b.example/3"), 10, one * 2), 0);
        assert_eq!(t.links.len(), 2);
    }

    #[test]
    fn truncating_forgets_the_newer_links_and_their_bytes() {
        let mut t = Table::default();
        t.add(Link::new("https://c.example/1"), 10, usize::MAX);
        let bytes = t.bytes;
        t.add(Link::new("https://c.example/2"), 10, usize::MAX);
        t.truncate(1);
        assert_eq!((t.links.len(), t.ids.len(), t.bytes), (1, 1, bytes));
        // The number comes back for a new link, and the old one keeps its own.
        assert_eq!(t.add(Link::new("https://c.example/3"), 10, usize::MAX), 2);
        assert_eq!(t.ids[&Link::new("https://c.example/1")], 1);
    }

    #[test]
    fn a_refused_link_is_counted_and_a_rollback_never_breaks_older_numbers() {
        let kept = intern(Link::new("https://d.example/kept"));
        let before = dropped();
        let m = mark();
        intern(Link::new(&"a".repeat(MAX_URL_BYTES + 1)));
        assert_eq!(dropped(), before + 1);
        let added = intern(Link::new("https://d.example/added"));
        // Other tests intern links at the same time, so whether this one
        // was taken back depends on them; either way the older link stays.
        if rollback(m) {
            assert!(get(added).is_none_or(|l| l.url != "https://d.example/added"));
        }
        assert_eq!(get(kept).unwrap().url, "https://d.example/kept");
    }
}
