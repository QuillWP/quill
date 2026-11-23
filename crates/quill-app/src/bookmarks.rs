//! Bookmarks for the files and folders Quill remembers between runs: recent
//! and last-session documents, extra font folders, the custom dictionary and
//! folders the person let Quill into.
//! In the App Sandbox a path alone does not let Quill open a file again after
//! a restart; the security-scoped bookmark made while the person's choice
//! still stood does. They are kept in a file beside the settings. Off the Mac
//! these do nothing.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The most bookmarks kept. Each is about 1 KB, and Quill wants at most a
/// hundred recent documents, ten font folders, fifty allowed folders and a
/// dictionary.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MAX: usize = 300;

/// Each path as Quill knows it, with the system's bookmark for it, newest
/// first.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
static STORE: Mutex<Vec<(PathBuf, Vec<u8>)>> = Mutex::new(Vec::new());

/// Remember a file or folder the person chose or opened, so Quill can open
/// it again in a later run. Call it while Quill may still open the item.
pub fn remember(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let Some(data) = mac::make(path) else {
            return;
        };
        let mut store = STORE.lock().unwrap_or_else(|e| e.into_inner());
        store.retain(|(p, _)| p != path);
        store.insert(0, (path.to_path_buf(), data));
        trim(&mut store, &wanted());
        write(&store);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = path;
}

/// Open access to everything remembered that the settings still name, and
/// forget the rest. Call it once at start, after the settings are loaded and
/// before anything they name is opened.
pub fn restore() {
    #[cfg(target_os = "macos")]
    {
        let Some(text) = file().and_then(|f| crate::settings::read_text(&f).ok()) else {
            return;
        };
        let wanted = wanted();
        let mut store = Vec::new();
        for (path, data) in from_text(&text) {
            if !wanted.contains(&path) {
                continue;
            }
            // One the system cannot resolve now (on a drive that is not
            // plugged in, say) is kept for a later run.
            let data = match mac::open(&data) {
                Some(Some(fresh)) => fresh,
                _ => data,
            };
            store.push((path, data));
        }
        trim(&mut store, &wanted);
        write(&store);
        *STORE.lock().unwrap_or_else(|e| e.into_inner()) = store;
    }
}

/// The paths the settings name, which are the ones worth a bookmark.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn wanted() -> std::collections::HashSet<PathBuf> {
    crate::settings::with(|s| {
        s.recent_documents
            .iter()
            .chain(&s.last_session)
            .chain(s.font_folders.iter().map(|f| &f.path))
            .chain(&s.custom_dictionary)
            .chain(&s.allowed_folders)
            .cloned()
            .collect()
    })
}

/// Bring `store` down to `MAX`, dropping the oldest the settings no longer
/// name first, so a long session of other documents cannot push out a
/// font folder or the dictionary.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn trim(store: &mut Vec<(PathBuf, Vec<u8>)>, wanted: &std::collections::HashSet<PathBuf>) {
    while store.len() > MAX {
        let at = store
            .iter()
            .rposition(|(p, _)| !wanted.contains(p))
            .unwrap_or(store.len() - 1);
        store.remove(at);
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn file() -> Option<PathBuf> {
    crate::settings::sibling("Bookmarks", "Quill Bookmarks.txt", "bookmarks")
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn write(store: &[(PathBuf, Vec<u8>)]) {
    if let Some(file) = file() {
        let _ = crate::settings::write_user_file(&file, to_text(store).as_bytes());
    }
}

/// One line each: the bookmark in base64, a tab, and the path written as the
/// settings file writes paths.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn to_text(store: &[(PathBuf, Vec<u8>)]) -> String {
    let mut out =
        String::from("# Quill's bookmarks for the files it remembers. Not for editing.\n");
    for (path, data) in store {
        out.push_str(&quill_formats::base64(data));
        out.push('\t');
        out.push_str(&crate::settings::escape_path(path));
        out.push('\n');
    }
    out
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn from_text(text: &str) -> Vec<(PathBuf, Vec<u8>)> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let (data, path) = line.split_once('\t')?;
            let data = quill_formats::base64_decode(data).filter(|d| !d.is_empty())?;
            (!path.is_empty()).then(|| (crate::settings::unescape_path(path), data))
        })
        .take(MAX)
        .collect()
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::autoreleasepool;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;
    use std::ffi::c_void;
    use std::path::Path;
    use std::ptr::{null, null_mut};

    // NSURLBookmarkCreationOptions and NSURLBookmarkResolutionOptions.
    const CREATE_WITH_SECURITY_SCOPE: usize = 1 << 11;
    const RESOLVE_WITHOUT_UI: usize = 1 << 8;
    const RESOLVE_WITHOUT_MOUNTING: usize = 1 << 9;
    const RESOLVE_WITH_SECURITY_SCOPE: usize = 1 << 10;

    /// A security-scoped bookmark for `path`, or None when the system will
    /// not make one (the item is gone, or Quill may not open it).
    pub fn make(path: &Path) -> Option<Vec<u8>> {
        let text = NSString::from_str(path.to_str()?);
        autoreleasepool(|_| {
            // SAFETY: Foundation class and instance methods with their
            // documented argument types; nil is checked before each use.
            unsafe {
                let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: &*text];
                bookmark_of(url)
            }
        })
    }

    /// SAFETY: `url` is nil or an NSURL.
    unsafe fn bookmark_of(url: *mut AnyObject) -> Option<Vec<u8>> {
        if url.is_null() {
            return None;
        }
        let mut error: *mut AnyObject = null_mut();
        // SAFETY: as the caller promises; nil keys and base are allowed.
        unsafe {
            let data: *mut AnyObject = msg_send![url,
                bookmarkDataWithOptions: CREATE_WITH_SECURITY_SCOPE
                includingResourceValuesForKeys: null::<AnyObject>()
                relativeToURL: null::<AnyObject>()
                error: &mut error];
            if data.is_null() {
                return None;
            }
            let len: usize = msg_send![data, length];
            let bytes: *const c_void = msg_send![data, bytes];
            let bytes = bytes.cast::<u8>();
            (!bytes.is_null() && len > 0).then(|| std::slice::from_raw_parts(bytes, len).to_vec())
        }
    }

    /// Resolve `data` and start access to what it names, for the rest of
    /// this run. None when it cannot be resolved; Some(Some(new)) when the
    /// system says the bookmark is out of date and gives a fresh one.
    pub fn open(data: &[u8]) -> Option<Option<Vec<u8>>> {
        autoreleasepool(|_| {
            // SAFETY: Foundation methods with their documented argument
            // types; the NSData copies `data`, and nil is checked first.
            unsafe {
                let data: *mut AnyObject = msg_send![class!(NSData),
                    dataWithBytes: data.as_ptr().cast::<c_void>()
                    length: data.len()];
                if data.is_null() {
                    return None;
                }
                let mut stale = Bool::NO;
                let mut error: *mut AnyObject = null_mut();
                let url: *mut AnyObject = msg_send![class!(NSURL),
                    URLByResolvingBookmarkData: data
                    options: RESOLVE_WITH_SECURITY_SCOPE | RESOLVE_WITHOUT_UI | RESOLVE_WITHOUT_MOUNTING
                    relativeToURL: null::<AnyObject>()
                    bookmarkDataIsStale: &mut stale
                    error: &mut error];
                if url.is_null() {
                    return None;
                }
                // Left open until Quill quits: these are the files it may
                // be asked to open at any time.
                let _: Bool = msg_send![url, startAccessingSecurityScopedResource];
                Some(if stale.as_bool() {
                    bookmark_of(url)
                } else {
                    None
                })
            }
        })
    }

    /// Where `data` points now, without starting access. For tests.
    #[cfg(test)]
    pub fn path_of(data: &[u8]) -> Option<std::path::PathBuf> {
        use std::ffi::{CStr, c_char};
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: as in `open`.
        autoreleasepool(|_| unsafe {
            let data: *mut AnyObject = msg_send![class!(NSData),
                dataWithBytes: data.as_ptr().cast::<c_void>()
                length: data.len()];
            let mut stale = Bool::NO;
            let mut error: *mut AnyObject = null_mut();
            let url: *mut AnyObject = msg_send![class!(NSURL),
                URLByResolvingBookmarkData: data
                options: RESOLVE_WITH_SECURITY_SCOPE | RESOLVE_WITHOUT_UI | RESOLVE_WITHOUT_MOUNTING
                relativeToURL: null::<AnyObject>()
                bookmarkDataIsStale: &mut stale
                error: &mut error];
            if url.is_null() {
                return None;
            }
            let path: *mut AnyObject = msg_send![url, path];
            let utf8: *const c_char = msg_send![path, fileSystemRepresentation];
            (!utf8.is_null()).then(|| {
                std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
                    CStr::from_ptr(utf8).to_bytes(),
                ))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_keeps_each_path_and_bookmark_exactly() {
        let store = vec![
            (PathBuf::from("/Users/pat/Report 1.qwp"), vec![0, 1, 2, 255]),
            (
                PathBuf::from("/Volumes/Disk/odd\tname%\n.docx"),
                vec![7; 1000],
            ),
        ];
        assert_eq!(from_text(&to_text(&store)), store);
        // Damaged lines are skipped, not taken as paths.
        let text = "nonsense\n\t/no/bookmark\n!!!\t/bad/base64\nAAEC\t/kept\n";
        assert_eq!(
            from_text(text),
            vec![(PathBuf::from("/kept"), vec![0, 1, 2])]
        );
    }

    #[test]
    fn trimming_drops_the_oldest_unwanted_first() {
        let wanted: std::collections::HashSet<PathBuf> = [PathBuf::from("/font folder")].into();
        let mut store: Vec<(PathBuf, Vec<u8>)> = (0..MAX + 2)
            .map(|n| (PathBuf::from(format!("/doc {n}")), vec![1]))
            .collect();
        // The font folder is the oldest of all, but the settings name it.
        store.push((PathBuf::from("/font folder"), vec![1]));
        trim(&mut store, &wanted);
        assert_eq!(store.len(), MAX);
        assert_eq!(store.last().unwrap().0, PathBuf::from("/font folder"));
        assert!(
            !store
                .iter()
                .any(|(p, _)| p == Path::new(&format!("/doc {}", MAX + 1)))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_bookmark_follows_its_file_and_opens_it() {
        if !crate::unrestricted_host_services() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("quill-bookmark-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Report.qwp");
        std::fs::write(&file, b"x").unwrap();
        let data = mac::make(&file).expect("a bookmark");
        assert!(mac::open(&data).is_some());
        // Renamed, it still finds the file.
        let moved = dir.join("Renamed.qwp");
        std::fs::rename(&file, &moved).unwrap();
        let found = mac::path_of(&data).expect("still resolves");
        assert_eq!(found.canonicalize().unwrap(), moved.canonicalize().unwrap());
        // Nothing is made for a file that is not there.
        assert!(mac::make(&dir.join("Missing.qwp")).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
