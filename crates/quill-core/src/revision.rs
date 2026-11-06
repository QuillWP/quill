//! Tracked changes. A run's `CharProps::rev` names an entry here.
//!
//! This is the same scheme as hyperlinks (`crate::link`): character
//! formats are small `Copy` values, so a run can't hold the change itself.
//! Every change Quill sees is kept once in a table shared by all open
//! documents and runs hold its number. Equal changes get the same number,
//! so neighboring runs of one change still merge, and undo, copy and the
//! layout cache need nothing new. Entries are never removed.

use crate::props::CharProps;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

/// Longest author name kept, in bytes. Word's own limit is 255 characters.
pub const MAX_AUTHOR: usize = 255;

/// Who made a change and when. The date is ISO 8601 as Word writes it,
/// "2026-10-23T14:05:00Z".
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Stamp {
    pub author: String,
    pub date: String,
}

impl Stamp {
    pub fn new(author: &str, date: &str) -> Stamp {
        Stamp {
            author: clip(author, MAX_AUTHOR).to_string(),
            date: clip(date, 32).to_string(),
        }
    }
}

fn clip(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// What happened to a run of text. Text can be inserted by one person and
/// deleted by another, and its format can have changed as well.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Revision {
    pub inserted: Option<Stamp>,
    pub deleted: Option<Stamp>,
    /// The character format changed: who changed it, and the format it
    /// had before (with no revision, object or link of its own).
    pub formatted: Option<(Stamp, CharProps)>,
}

impl Revision {
    pub fn is_empty(&self) -> bool {
        self.inserted.is_none() && self.deleted.is_none() && self.formatted.is_none()
    }
}

#[derive(Default)]
struct Table {
    revisions: Vec<Arc<Revision>>,
    ids: HashMap<Arc<Revision>, u32>,
}

fn table() -> &'static RwLock<Table> {
    static TABLE: OnceLock<RwLock<Table>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

/// The number for `rev`, adding it to the table if it is new. An empty
/// revision is 0, which means "no change".
pub fn intern(mut rev: Revision) -> u32 {
    if rev.is_empty() {
        return 0;
    }
    // A format change keeps formatting only: what the text is (a picture,
    // a note's mark) and its link are not part of it, and Reject leaves
    // them as they are.
    if let Some((_, old)) = &mut rev.formatted {
        old.rev = 0;
        old.object = 0;
        old.link = 0;
        old.comment = 0;
    }
    if let Some(&id) = table()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .ids
        .get(&rev)
    {
        return id;
    }
    let mut t = table().write().unwrap_or_else(|e| e.into_inner());
    if let Some(&id) = t.ids.get(&rev) {
        return id;
    }
    let rev = Arc::new(rev);
    t.revisions.push(rev.clone());
    let id = t.revisions.len() as u32;
    t.ids.insert(rev, id);
    id
}

/// The revision numbered `id`, or None for 0 and numbers never handed out.
pub fn get(id: u32) -> Option<Arc<Revision>> {
    let i = (id as usize).checked_sub(1)?;
    table()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .revisions
        .get(i)
        .cloned()
}

/// Revision `id` changed by `f`, as a number.
pub fn update(id: u32, f: impl FnOnce(&mut Revision)) -> u32 {
    let mut rev = get(id).map(|r| (*r).clone()).unwrap_or_default();
    f(&mut rev);
    intern(rev)
}

pub fn is_inserted(id: u32) -> bool {
    get(id).is_some_and(|r| r.inserted.is_some())
}

pub fn is_deleted(id: u32) -> bool {
    get(id).is_some_and(|r| r.deleted.is_some())
}

/// The color an author's changes are shown in: one of eight dark,
/// readable colors, always the same for the same name.
pub fn author_color(author: &str) -> crate::props::Rgb {
    const COLORS: [u32; 8] = [
        0xC00000, 0x0050C0, 0x007A00, 0x8000A0, 0xB05000, 0x007070, 0xA00060, 0x606000,
    ];
    // FNV-1a: small, and stable from one run of Quill to the next.
    let mut h: u32 = 0x811C_9DC5;
    for b in author.bytes() {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    crate::props::Rgb(COLORS[h as usize % COLORS.len()])
}

/// The clock that dates changes. Quill's app sets one that gives local
/// time, as Word writes it; the default is UTC.
static CLOCK: OnceLock<fn() -> String> = OnceLock::new();

pub fn set_clock(clock: fn() -> String) {
    let _ = CLOCK.set(clock);
}

/// Where the editing author's name comes from when a document doesn't
/// name one: Quill's app sets Preferences' Your Name.
static AUTHOR: OnceLock<fn() -> String> = OnceLock::new();

pub fn set_author_source(author: fn() -> String) {
    let _ = AUTHOR.set(author);
}

/// The name new changes are signed with, if the app gave one.
pub fn default_author() -> String {
    AUTHOR.get().map(|f| f()).unwrap_or_default()
}

/// Now, to the minute, as a change's date.
pub fn now() -> String {
    match CLOCK.get() {
        Some(clock) => clock(),
        None => utc_now(),
    }
}

fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:00Z",
        s / 3600,
        s % 3600 / 60
    )
}

/// A calendar date to days since 1970-01-01 (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_revisions_share_a_number_and_empty_ones_are_none() {
        let ada = Stamp::new("Ada", "2026-10-23T10:00:00Z");
        let a = intern(Revision {
            inserted: Some(ada.clone()),
            ..Revision::default()
        });
        let b = intern(Revision {
            inserted: Some(ada.clone()),
            ..Revision::default()
        });
        assert_eq!(a, b);
        assert_ne!(a, 0);
        assert!(is_inserted(a) && !is_deleted(a));
        let c = update(a, |r| {
            r.deleted = Some(Stamp::new("Bo", "2026-10-23T11:00:00Z"))
        });
        assert!(is_inserted(c) && is_deleted(c));
        assert_eq!(intern(Revision::default()), 0);
        assert_eq!(update(a, |r| r.inserted = None), 0);
        assert!(get(0).is_none() && get(u32::MAX).is_none());
    }

    #[test]
    fn long_names_are_cut_at_a_character_boundary() {
        let s = Stamp::new(&"é".repeat(200), "2026");
        assert!(s.author.len() <= MAX_AUTHOR && s.author.chars().all(|c| c == 'é'));
    }

    #[test]
    fn dates_look_like_words() {
        let d = utc_now();
        assert_eq!(d.len(), 20, "{d}");
        assert!(d.ends_with(":00Z") && d.as_bytes()[10] == b'T');
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_749), (2026, 10, 23));
        assert_eq!(days_from_civil(2026, 10, 23), 20_749);
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }
}
