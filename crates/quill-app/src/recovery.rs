//! Atomic autosave files and startup crash recovery.

use quill_core::Document;
use std::path::{Path, PathBuf};

const EXTENSION: &str = "wrec";

fn directory() -> Option<PathBuf> {
    crate::settings::sibling("Quill Recovery", "Recovery", "recovery")
}

fn safe_title(title: &str) -> String {
    let title: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let title = title.trim();
    if title.is_empty() {
        "Untitled".into()
    } else {
        title.into()
    }
}

fn new_path(dir: &Path, title: &str, serial: u64) -> PathBuf {
    dir.join(format!(
        "{}-{serial}-{}.{}",
        std::process::id(),
        safe_title(title),
        EXTENSION
    ))
}

fn title_from_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    stem.splitn(3, '-').nth(2).unwrap_or("Untitled").to_string()
}

/// Refresh one document's recovery copy, creating its stable slot on the
/// first autosave.
pub fn save(
    current: Option<&Path>,
    doc: &Document,
    title: &str,
    serial: u64,
) -> Result<PathBuf, String> {
    let path = match current {
        Some(path) => path.to_path_buf(),
        None => new_path(
            &directory().ok_or_else(|| "Quill's recovery folder is unavailable.".to_string())?,
            title,
            serial,
        ),
    };
    let bytes = quill_formats::native::write(
        doc,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    );
    crate::settings::write_user_file(&path, &bytes)?;
    Ok(path)
}

/// Recovery files left by an earlier process. Damaged files remain on disk
/// for manual salvage; valid ones retain their slot until saved or closed.
pub fn load_all() -> Vec<(PathBuf, String, Document)> {
    let Some(dir) = directory() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| match quill_formats::load(&path) {
            Ok(mut doc) => {
                doc.touch();
                let title = title_from_path(&path);
                Some((path, title, doc))
            }
            Err(e) => {
                eprintln!("quill: could not recover {}: {e}", path.display());
                None
            }
        })
        .collect()
}

pub fn remove(path: Option<PathBuf>) {
    if let Some(path) = path {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_names_are_safe_and_keep_a_readable_title() {
        let dir = Path::new("/tmp/recovery");
        let path = new_path(dir, "Draft/One: revised.qwp", 7);
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("wrec"));
        assert_eq!(title_from_path(&path), "Draft_One_ revised_qwp");
    }

    #[test]
    fn recovery_copy_is_a_loadable_native_document_and_replaces_atomically() {
        let dir = std::env::temp_dir().join(format!("quill-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("slot.wrec");
        let first = quill_core::text::from_plain_text("First draft");
        let second = quill_core::text::from_plain_text("Second draft");

        save(Some(&path), &first, "Draft", 1).unwrap();
        save(Some(&path), &second, "Draft", 1).unwrap();

        let back = quill_formats::load(&path).unwrap();
        assert_eq!(back.main.paras[0].text, "Second draft");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        remove(Some(path));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
