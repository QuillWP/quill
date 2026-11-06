//! Hyphenation and thesaurus data loaded from the `data` folder, plus the
//! session's Ignore All list. Spelling itself comes from the host system.

use quill_core::hyphen::Hyphenator;
use quill_core::thesaurus::Thesaurus;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Where the data files may be: beside the program, in a macOS bundle's
/// Resources, or (when run from a build) in the source tree.
fn data_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        out.push(dir.join("data"));
        out.push(dir.join("../Resources/data"));
        out.push(dir.join("../../data"));
        out.push(dir.join("../../../data"));
    }
    out.push(PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data"
    )));
    out
}

/// A data file's text, inflated.
fn load(name: &str) -> Option<String> {
    let bytes = data_dirs()
        .into_iter()
        .find_map(|d| std::fs::read(d.join(name)).ok())?;
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&bytes).ok()?;
    String::from_utf8(raw).ok()
}

pub fn hyphenator() -> Option<&'static Hyphenator> {
    static H: OnceLock<Option<Hyphenator>> = OnceLock::new();
    H.get_or_init(|| load("hyph_en_US.pat").map(|t| Hyphenator::from_text(&t)))
        .as_ref()
}

pub fn thesaurus() -> Option<&'static Thesaurus> {
    static T: OnceLock<Option<Thesaurus>> = OnceLock::new();
    T.get_or_init(|| load("th_en_US.dat").map(Thesaurus::from_text))
        .as_ref()
}

static IGNORED: Mutex<Vec<String>> = Mutex::new(Vec::new());

thread_local! {
    static LOCAL: RefCell<Option<Result<crate::local_dictionary::Dictionary, String>>> = const { RefCell::new(None) };
}

fn with_local<R>(
    f: impl FnOnce(&mut Result<crate::local_dictionary::Dictionary, String>) -> R,
) -> R {
    LOCAL.with(|local| {
        let mut local = local.borrow_mut();
        let dict = local.get_or_insert_with(|| match crate::local_dictionary::path() {
            Some(path) => crate::local_dictionary::Dictionary::load(&path),
            None => Ok(crate::local_dictionary::Dictionary::default()),
        });
        f(dict)
    })
}

pub fn reload_dictionary() {
    LOCAL.with(|local| *local.borrow_mut() = None);
}

pub fn prepare_dictionary() -> Result<(), String> {
    with_local(|dict| dict.as_ref().map(|_| ()).map_err(Clone::clone))
}

/// Whether `word` is accepted locally, by the host spelling service, or by Ignore All.
/// With no system provider everything passes.
pub fn word_ok(word: &str) -> bool {
    if with_local(|dict| dict.as_ref().is_ok_and(|dict| dict.contains(word))) {
        return true;
    }
    if IGNORED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|w| w == word)
    {
        return true;
    }
    crate::spellcheck::check(word).unwrap_or(true)
}

/// Add in the Spelling window defaults to Quill's own persistent dictionary.
pub fn add_word(word: &str) -> Result<(), String> {
    let path =
        crate::local_dictionary::path().ok_or("The Quill dictionary folder is unavailable.")?;
    with_local(|dict| match dict {
        Ok(dict) => dict.add(&path, word),
        Err(error) => Err(error.clone()),
    })
}

/// Ignore All: pass over `word` for the rest of the session.
pub fn ignore_all(word: &str) {
    IGNORED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(word.to_string());
}

/// Preferences > Spelling > Reset "Ignore All" List.
pub fn reset_ignored() {
    IGNORED.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_files_load_and_work() {
        let h = hyphenator().expect("hyphenation patterns");
        assert_eq!(h.breaks("hyphenation"), [2, 6]);
        let t = thesaurus().expect("thesaurus");
        let (head, m) = t.look_up("happier").unwrap();
        assert_eq!(head, "happy");
        assert!(m.iter().any(|m| m.words.iter().any(|w| w == "glad")));
    }
}
