//! Hyphenation from macOS itself (Core Foundation), in the language the
//! system is set to, or US English when that language has none. The Mac
//! build needs no patterns file of its own.

use std::ffi::c_void;

type CFIndex = isize;
type Ref = *const c_void;

#[repr(C)]
struct CFRange {
    location: CFIndex,
    length: CFIndex,
}

const NOT_FOUND: CFIndex = -1;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCharacters(alloc: Ref, chars: *const u16, n: CFIndex) -> Ref;
    fn CFStringCreateWithBytes(
        alloc: Ref,
        bytes: *const u8,
        n: CFIndex,
        encoding: u32,
        external: u8,
    ) -> Ref;
    fn CFLocaleCopyCurrent() -> Ref;
    fn CFLocaleCreate(alloc: Ref, id: Ref) -> Ref;
    fn CFStringIsHyphenationAvailableForLocale(locale: Ref) -> u8;
    fn CFStringGetHyphenationLocationBeforeIndex(
        s: Ref,
        location: CFIndex,
        limit: CFRange,
        options: usize,
        locale: Ref,
        character: *mut u32,
    ) -> CFIndex;
    fn CFRelease(r: Ref);
}

const UTF8: u32 = 0x0800_0100;

/// The locale to hyphenate in, kept for the whole run.
struct Locale(Ref);
// The locale is created once, never changed and only read.
unsafe impl Send for Locale {}
unsafe impl Sync for Locale {}

fn locale() -> Option<Ref> {
    static L: std::sync::OnceLock<Option<Locale>> = std::sync::OnceLock::new();
    L.get_or_init(|| unsafe {
        let current = CFLocaleCopyCurrent();
        if !current.is_null() && CFStringIsHyphenationAvailableForLocale(current) != 0 {
            return Some(Locale(current));
        }
        if !current.is_null() {
            CFRelease(current);
        }
        let id = b"en_US";
        let name = CFStringCreateWithBytes(std::ptr::null(), id.as_ptr(), 5, UTF8, 0);
        if name.is_null() {
            return None;
        }
        let us = CFLocaleCreate(std::ptr::null(), name);
        CFRelease(name);
        (!us.is_null() && CFStringIsHyphenationAvailableForLocale(us) != 0).then_some(Locale(us))
    })
    .as_ref()
    .map(|l| l.0)
}

pub fn available() -> bool {
    locale().is_some()
}

/// Where `word` may break, as character counts from its start, leaving at
/// least two letters before a break and three after, as the patterns do.
pub fn breaks(word: &str) -> Vec<usize> {
    let Some(locale) = locale() else {
        return Vec::new();
    };
    let n = word.chars().count();
    if n < 5 || word.chars().any(|c| !c.is_alphabetic()) {
        return Vec::new();
    }
    let units: Vec<u16> = word.encode_utf16().collect();
    // Character count before each UTF-16 position.
    let mut chars_at = vec![0usize; units.len() + 1];
    let mut k = 0;
    for (i, c) in word.chars().enumerate() {
        for _ in 0..c.len_utf16() {
            chars_at[k] = i;
            k += 1;
        }
    }
    chars_at[units.len()] = n;
    let mut out = Vec::new();
    unsafe {
        let s =
            CFStringCreateWithCharacters(std::ptr::null(), units.as_ptr(), units.len() as CFIndex);
        if s.is_null() {
            return out;
        }
        let limit = || CFRange {
            location: 0,
            length: units.len() as CFIndex,
        };
        let mut at = units.len() as CFIndex;
        while at > 0 {
            let b = CFStringGetHyphenationLocationBeforeIndex(
                s,
                at,
                limit(),
                0,
                locale,
                std::ptr::null_mut(),
            );
            if b == NOT_FOUND || b <= 0 || b >= at {
                break;
            }
            let c = chars_at[b as usize];
            if c >= 2 && n - c >= 3 {
                out.push(c);
            }
            at = b;
        }
        CFRelease(s);
    }
    out.sort_unstable();
    out.dedup();
    out
}
