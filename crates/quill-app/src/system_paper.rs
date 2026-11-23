//! The paper sizes the computer offers, for the paper pop-ups in Document
//! Setup, Page Setup and Preferences.
//!
//! On the Mac these come from the default printer (or the first printer
//! that has any), through the PrintCore API. Other systems, and a Mac with
//! no printer, get an empty list and the caller falls back to its own table.

/// A paper size: name, then width and height in twips, portrait.
pub type Paper = (String, i32, i32);

/// The system's paper sizes, smallest first. Empty if none can be read.
pub fn system_papers() -> Vec<Paper> {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        use std::sync::OnceLock;
        // Asking the print system is slow the first time, and the list does
        // not change while Quill runs.
        static LIST: OnceLock<Vec<Paper>> = OnceLock::new();
        return LIST.get_or_init(mac::read).clone();
    }
    #[allow(unreachable_code)]
    Vec::new()
}

/// The custom sizes defined in the system (the Mac's "Manage Custom Sizes"),
/// sorted like the standard ones. Empty if there are none.
pub fn custom_papers() -> Vec<Paper> {
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::read_custom();
    #[allow(unreachable_code)]
    Vec::new()
}

/// Compare names the way the Print panel lists them: ignoring case, with
/// runs of digits compared as numbers ("A4" before "A10", "4 x 6" before
/// "8.5 x 13").
pub(crate) fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (a, b): (Vec<char>, Vec<char>) = (
        a.to_lowercase().chars().collect(),
        b.to_lowercase().chars().collect(),
    );
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (si, sj) = (i, j);
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let (x, y): (String, String) = (a[si..i].iter().collect(), b[sj..j].iter().collect());
            let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
            let o = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
            if o != Ordering::Equal {
                return o;
            }
        } else {
            // Digits come before letters and symbols.
            let (da, db) = (a[i].is_ascii_digit(), b[j].is_ascii_digit());
            if da != db {
                return if da {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            if a[i] != b[j] {
                return a[i].cmp(&b[j]);
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

/// Sort, drop repeats and anything unusable for a page. Printers list the
/// same size several times (borderless, with and without a tray).
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
fn tidy(raw: Vec<(String, f64, f64)>) -> Vec<Paper> {
    let mut out: Vec<Paper> = Vec::new();
    for (name, w, h) in raw {
        let (w, h) = ((w * 20.0).round(), (h * 20.0).round());
        let (w, h) = (w.min(h), w.max(h));
        let range = quill_core::limits::MIN_PAGE as f64..=quill_core::limits::MAX_LENGTH as f64;
        if !(range.contains(&w) && range.contains(&h)) {
            continue;
        }
        let lower = name.to_lowercase();
        if lower.contains("borderless") || lower.contains("custom") || name.trim().is_empty() {
            continue;
        }
        let (w, h) = (w as i32, h as i32);
        if out
            .iter()
            .any(|p| (p.1 - w).abs() < 20 && (p.2 - h).abs() < 20)
        {
            continue;
        }
        out.push((name, w, h));
    }
    out.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    out
}

/// The user's own sizes: any name, in range, sorted by name.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
fn tidy_custom(raw: Vec<(String, f64, f64)>) -> Vec<Paper> {
    let range = quill_core::limits::MIN_PAGE as f64..=quill_core::limits::MAX_LENGTH as f64;
    let mut out: Vec<Paper> = raw
        .into_iter()
        .filter_map(|(name, w, h)| {
            let (w, h) = ((w * 20.0).round(), (h * 20.0).round());
            let (w, h) = (w.min(h), w.max(h));
            (range.contains(&w) && range.contains(&h) && !name.trim().is_empty())
                .then_some((name, w as i32, h as i32))
        })
        .collect();
    out.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    out
}

#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
mod mac {
    use super::{Paper, tidy};
    use std::ffi::{c_char, c_void};

    type CFTypeRef = *const c_void;
    type CFIndex = isize;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn PMServerCreatePrinterList(server: CFTypeRef, list: *mut CFTypeRef) -> i32;
        fn PMPrinterIsDefault(printer: CFTypeRef) -> u8;
        fn PMPrinterGetPaperList(printer: CFTypeRef, list: *mut CFTypeRef) -> i32;
        fn PMPaperGetWidth(paper: CFTypeRef, width: *mut f64) -> i32;
        fn PMPaperGetHeight(paper: CFTypeRef, height: *mut f64) -> i32;
        fn PMPaperCreateLocalizedName(
            paper: CFTypeRef,
            printer: CFTypeRef,
            name: *mut CFTypeRef,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFArrayGetCount(array: CFTypeRef) -> CFIndex;
        fn CFArrayGetValueAtIndex(array: CFTypeRef, i: CFIndex) -> CFTypeRef;
        fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: CFIndex, encoding: u32) -> u8;
        fn CFRelease(cf: CFTypeRef);
    }

    const UTF8: u32 = 0x0800_0100;

    unsafe fn string(s: CFTypeRef) -> Option<String> {
        let mut buf = [0 as c_char; 256];
        // SAFETY: the buffer length is passed, and s is a live CFString.
        if unsafe { CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as CFIndex, UTF8) } == 0 {
            return None;
        }
        // SAFETY: CFStringGetCString wrote a NUL-terminated string.
        let c = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) };
        Some(c.to_string_lossy().into_owned())
    }

    /// Papers of one printer, as (name, width pt, height pt).
    unsafe fn papers_of(printer: CFTypeRef) -> Vec<(String, f64, f64)> {
        let mut out = Vec::new();
        let mut list: CFTypeRef = std::ptr::null();
        // SAFETY: the list is owned by the printer and is not released here.
        if unsafe { PMPrinterGetPaperList(printer, &mut list) } != 0 || list.is_null() {
            return out;
        }
        for i in 0..unsafe { CFArrayGetCount(list) } {
            let paper = unsafe { CFArrayGetValueAtIndex(list, i) };
            let (mut w, mut h) = (0.0f64, 0.0f64);
            let mut name: CFTypeRef = std::ptr::null();
            unsafe {
                if PMPaperGetWidth(paper, &mut w) != 0
                    || PMPaperGetHeight(paper, &mut h) != 0
                    || PMPaperCreateLocalizedName(paper, printer, &mut name) != 0
                    || name.is_null()
                {
                    continue;
                }
                if let Some(n) = string(name) {
                    out.push((n, w, h));
                }
                CFRelease(name);
            }
        }
        out
    }

    /// Sizes from the user's custom paper list, which the Print panel's
    /// "Manage Custom Sizes" keeps in the preferences domain
    /// com.apple.print.custompapers (points). Read through the preferences
    /// system rather than as a file, so it does not depend on HOME and the
    /// App Sandbox can allow it; where it does not, the list is empty.
    pub fn read_custom() -> Vec<Paper> {
        use objc2::runtime::{AnyObject, Bool};
        use objc2::{class, msg_send};
        use objc2_foundation::NSString;
        let domain = NSString::from_str("com.apple.print.custompapers");
        let mut raw = Vec::new();
        // SAFETY: plain Foundation reads; every value is checked for its
        // class before it is used.
        unsafe {
            let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
            if defaults.is_null() {
                return Vec::new();
            }
            let dict: *mut AnyObject = msg_send![defaults, persistentDomainForName: &*domain];
            if dict.is_null() {
                return Vec::new();
            }
            let values: *mut AnyObject = msg_send![dict, allValues];
            let count: usize = msg_send![values, count];
            for i in 0..count {
                let entry: *mut AnyObject = msg_send![values, objectAtIndex: i];
                let is_dict: Bool = msg_send![entry, isKindOfClass: class!(NSDictionary)];
                if !is_dict.as_bool() {
                    continue;
                }
                let get = |key: &str| -> *mut AnyObject {
                    let key = NSString::from_str(key);
                    msg_send![entry, objectForKey: &*key]
                };
                let number = |v: *mut AnyObject| -> Option<f64> {
                    if v.is_null() {
                        return None;
                    }
                    let ok: Bool = msg_send![v, isKindOfClass: class!(NSNumber)];
                    ok.as_bool().then(|| msg_send![v, doubleValue])
                };
                let name = get("name");
                if name.is_null() {
                    continue;
                }
                let ok: Bool = msg_send![name, isKindOfClass: class!(NSString)];
                if !ok.as_bool() {
                    continue;
                }
                let c: *const c_char = msg_send![name, UTF8String];
                if c.is_null() {
                    continue;
                }
                let name = std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned();
                if let (Some(w), Some(h)) = (number(get("width")), number(get("height"))) {
                    raw.push((name, w, h));
                }
            }
        }
        super::tidy_custom(raw)
    }

    pub fn read() -> Vec<Paper> {
        let mut printers: CFTypeRef = std::ptr::null();
        // SAFETY: a null server is the local one; the list is released below.
        unsafe {
            if PMServerCreatePrinterList(std::ptr::null(), &mut printers) != 0 || printers.is_null()
            {
                return Vec::new();
            }
            let count = CFArrayGetCount(printers);
            let all: Vec<CFTypeRef> = (0..count)
                .map(|i| CFArrayGetValueAtIndex(printers, i))
                .collect();
            // The default printer first, then the others until one has sizes.
            let default = all.iter().position(|&p| PMPrinterIsDefault(p) != 0);
            let order = default
                .into_iter()
                .chain((0..all.len()).filter(|&i| Some(i) != default));
            let mut papers = Vec::new();
            for i in order {
                papers = tidy(papers_of(all[i]));
                if !papers.is_empty() {
                    break;
                }
            }
            CFRelease(printers);
            papers
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_orders_and_drops_repeats() {
        let raw = vec![
            ("A4".to_string(), 595.0, 842.0),
            ("Letter".to_string(), 612.0, 792.0),
            ("A4 (Borderless)".to_string(), 595.0, 842.0),
            ("A4 again".to_string(), 842.0, 595.0),
            ("Custom".to_string(), 500.0, 700.0),
            ("Tiny".to_string(), 10.0, 10.0),
        ];
        let v = tidy(raw);
        let names: Vec<&str> = v.iter().map(|p| p.0.as_str()).collect();
        assert_eq!(names, ["A4", "Letter"]);
        assert_eq!((v[1].1, v[1].2), (12240, 15840));
    }

    #[test]
    fn natural_order_matches_the_print_panel() {
        let mut v = vec![
            "US Legal",
            "A4",
            "8.5 x 13",
            "Envelope C5",
            "A10",
            "Envelope #10",
            "4 x 6",
            "5 x 7",
            "a5",
            "Oficio",
        ];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            v,
            [
                "4 x 6",
                "5 x 7",
                "8.5 x 13",
                "A4",
                "a5",
                "A10",
                "Envelope #10",
                "Envelope C5",
                "Oficio",
                "US Legal"
            ]
        );
    }

    /// Prints what this Mac offers. Run by hand:
    /// cargo test -p quill-app system_paper -- --ignored --nocapture
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn mac_lists_papers() {
        let v = super::mac::read();
        for p in v.iter().chain(&super::mac::read_custom()) {
            println!("{} {}x{}", p.0, p.1, p.2);
        }
        assert!(!v.is_empty());
    }
}
