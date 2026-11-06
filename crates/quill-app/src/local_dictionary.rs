//! Quill-only learned words. Reading and writing never changes the OS dictionary.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_WORDS: usize = 10_000;

#[derive(Default)]
pub struct Dictionary {
    words: BTreeSet<String>,
}

fn word_key(word: &str) -> Result<String, String> {
    let key = word.to_lowercase();
    if word.is_empty()
        || word.len() > 256
        || key.len() > 256
        || word.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("A dictionary entry must be one word of at most 256 UTF-8 bytes.".into());
    }
    Ok(key)
}

impl Dictionary {
    pub fn load(path: &Path) -> Result<Self, String> {
        // Look at the path before opening it: opening a named pipe can
        // block, and the reader below refuses anything but a regular file.
        match std::fs::metadata(path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("Could not read “{}”: {e}", path.display())),
        }
        let bytes = quill_formats::read_file_with_limit(path, MAX_BYTES as u64)?;
        Self::parse(&bytes)
            .map_err(|e| format!("Invalid Quill dictionary “{}”: {e}", path.display()))
    }

    fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES {
            return Err("The dictionary exceeds 2 MB.".into());
        }
        let text =
            std::str::from_utf8(bytes).map_err(|_| "The dictionary must contain UTF-8 text.")?;
        let mut words = BTreeSet::new();
        for line in text
            .trim_start_matches('\u{feff}')
            .lines()
            .filter(|line| !line.is_empty())
        {
            words.insert(word_key(line)?);
            if words.len() > MAX_WORDS {
                return Err("The dictionary exceeds 10,000 words.".into());
            }
        }
        Ok(Self { words })
    }

    pub fn contains(&self, word: &str) -> bool {
        self.words.contains(&word.to_lowercase())
    }

    pub fn add(&mut self, path: &Path, word: &str) -> Result<(), String> {
        let key = word_key(word)?;
        if self.words.contains(&key) {
            return Ok(());
        }
        if self.words.len() >= MAX_WORDS {
            return Err("The Quill dictionary already contains 10,000 words.".into());
        }
        let mut next = self.words.clone();
        next.insert(key);
        let text = next
            .iter()
            .map(|word| format!("{word}\n"))
            .collect::<String>();
        if text.len() > MAX_BYTES {
            return Err("The Quill dictionary would exceed 2 MB.".into());
        }
        // Commit memory only after the atomic write succeeds.
        crate::settings::write_user_file(path, text.as_bytes())?;
        self.words = next;
        Ok(())
    }
}

pub fn path() -> Option<PathBuf> {
    crate::settings::with(|s| s.custom_dictionary.clone()).or_else(|| {
        crate::settings::sibling(
            "Quill Dictionary.txt",
            "Quill Dictionary.txt",
            "dictionary.txt",
        )
    })
}

/// Validate before switching. A failed selection never replaces the active dictionary.
fn prepare_file(path: &Path, create: bool) -> Result<(), String> {
    if create {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| {
                format!(
                    "Could not create “{}”: {e}. To use an existing dictionary, choose Choose….",
                    path.display()
                )
            })?
            .sync_all()
            .map_err(|e| e.to_string())?;
    } else if !path.is_file() {
        return Err("Choose an existing UTF-8 word list, with one word per line.".into());
    }
    Dictionary::load(path)?;
    Ok(())
}

#[cfg(test)]
pub fn select_file(path: &Path, create: bool) -> Result<(), String> {
    prepare_file(path, create)?;
    crate::settings::update(|s| {
        s.custom_dictionary = Some(path.to_path_buf());
        s.words_to_system = false;
    });
    crate::language::reload_dictionary();
    Ok(())
}

pub fn choose_file(create: bool) -> Result<Option<PathBuf>, String> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(if create {
            "New Custom Dictionary"
        } else {
            "Choose Custom Dictionary"
        })
        .add_filter("UTF-8 Word Lists", &["txt", "dic"]);
    if let Some(path) = path()
        && let Some(parent) = path.parent()
    {
        dialog = dialog.set_directory(parent);
    }
    let chosen = if create {
        dialog.set_file_name("Custom Dictionary.txt").save_file()
    } else {
        dialog.pick_file()
    };
    if let Some(path) = &chosen {
        prepare_file(path, create)?;
    }
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_and_creating_dictionaries_switches_words_without_overwriting_files() {
        let original = crate::settings::get();
        let dir =
            std::env::temp_dir().join(format!("quill-dictionary-selection-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("first.txt");
        let second = dir.join("second.txt");
        let invalid = dir.join("invalid.txt");
        std::fs::write(&first, "ZzxqLucasLexicon\n").unwrap();
        std::fs::write(&invalid, [0xff]).unwrap();
        select_file(&first, false).unwrap();
        assert_eq!(path(), Some(first.clone()));
        assert!(crate::language::word_ok("ZzxqLucasLexicon"));
        assert!(select_file(&invalid, false).is_err());
        assert!(select_file(&first, true).is_err());
        assert_eq!(path(), Some(first.clone()));
        assert_eq!(
            std::fs::read_to_string(&first).unwrap(),
            "ZzxqLucasLexicon\n"
        );
        select_file(&second, true).unwrap();
        assert_eq!(path(), Some(second.clone()));
        assert!(!crate::settings::get().words_to_system);
        crate::language::add_word("ZzxqNewLexicon").unwrap();
        assert!(
            Dictionary::load(&second)
                .unwrap()
                .contains("zzxqnewlexicon")
        );
        assert!(
            !Dictionary::load(&second)
                .unwrap()
                .contains("ZzxqLucasLexicon")
        );
        crate::settings::update(|s| *s = original);
        crate::language::reload_dictionary();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn local_words_survive_reopening_and_failed_writes_do_not_learn() {
        let dir = std::env::temp_dir().join(format!("quill-dictionary-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.txt");
        let mut dict = Dictionary::default();
        dict.add(&path, "Krupinski").unwrap();
        assert!(Dictionary::load(&path).unwrap().contains("KRUPINSKI"));
        assert!(
            dict.add(&path.join("not-a-directory"), "NeverLearned")
                .is_err()
        );
        assert!(!dict.contains("NeverLearned"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_named_pipe_is_refused_instead_of_blocking() {
        let dir = std::env::temp_dir().join(format!("quill-dict-pipe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pipe = dir.join("words.txt");
        let made = std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap();
        assert!(made.success());
        // Nothing writes to the pipe; opening it for reading would hang.
        let error = Dictionary::load(&pipe)
            .err()
            .expect("a pipe is not a dictionary");
        assert!(error.contains("Not a regular file"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_dictionary_past_the_limit_is_refused_unread() {
        let dir = std::env::temp_dir().join(format!("quill-dict-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("words.txt");
        std::fs::write(&big, vec![b'a'; MAX_BYTES + 1]).unwrap();
        assert!(Dictionary::load(&big).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_files_are_rejected_instead_of_replaced_by_empty_dictionaries() {
        assert!(Dictionary::parse(&[0xff]).is_err());
        assert!(Dictionary::parse(b"two words\n").is_err());
        assert!(
            word_key(&"İ".repeat(100)).is_err(),
            "case folding must not create unreadable entries"
        );
        assert!(Dictionary::parse(&vec![b'a'; MAX_BYTES + 1]).is_err());
        let dict = Dictionary::parse("\u{feff}Quill\r\nquill\r\n".as_bytes()).unwrap();
        assert_eq!(dict.words.len(), 1);
    }
}
