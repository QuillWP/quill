//! Configurable recent documents, persisted by Quill on every platform.
//! macOS also updates NSDocumentController for system recent-document support.

use std::path::{Path, PathBuf};

/// Quill remembers up to 100 documents however many the menu shows, so
/// removing one from the menu brings the next in line into view.
const REMEMBERED: usize = crate::settings::MAX_RECENT_DOCUMENTS;

fn move_to_front(paths: &mut Vec<PathBuf>, path: &Path) {
    paths.retain(|old| old != path);
    paths.insert(0, path.to_path_buf());
    paths.truncate(REMEMBERED);
}

#[cfg(target_os = "macos")]
fn controller() -> Option<objc2_06::rc::Retained<objc2_app_kit_03::NSDocumentController>> {
    let mtm = objc2_foundation_03::MainThreadMarker::new()?;
    Some(objc2_app_kit_03::NSDocumentController::sharedDocumentController(mtm))
}

pub fn load() -> Vec<PathBuf> {
    let mut paths = crate::settings::with(|s| s.recent_documents.clone());
    #[cfg(target_os = "macos")]
    if paths.is_empty()
        && let Some(controller) = controller()
    {
        let urls = controller.recentDocumentURLs();
        paths = (0..urls.count().min(REMEMBERED))
            .filter_map(|i| urls.objectAtIndex(i).path())
            .map(|p| PathBuf::from(p.to_string()))
            .collect();
    }
    paths.truncate(REMEMBERED);
    paths
}

pub fn note(paths: &mut Vec<PathBuf>, path: &Path) {
    move_to_front(paths, path);
    #[cfg(target_os = "macos")]
    if let Some(controller) = controller() {
        use objc2_foundation_03::{NSString, NSURL};
        let path = NSString::from_str(&path.to_string_lossy());
        controller.noteNewRecentDocumentURL(&NSURL::fileURLWithPath(&path));
    }
    crate::settings::update(|settings| settings.recent_documents = paths.clone());
}

/// Take entry `index` out of the list. The system's own recent list has no
/// way to drop one document, so Quill's list is the one that counts.
pub fn remove(paths: &mut Vec<PathBuf>, index: usize) {
    if index < paths.len() {
        paths.remove(index);
        crate::settings::update(|settings| settings.recent_documents = paths.clone());
    }
}

pub fn clear(paths: &mut Vec<PathBuf>) {
    paths.clear();
    #[cfg(target_os = "macos")]
    if let Some(controller) = controller() {
        // SAFETY: nil is the documented sender for a programmatic clear.
        unsafe { controller.clearRecentDocuments(None) };
    }
    crate::settings::update(|settings| settings.recent_documents.clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_paths_are_unique_newest_first_and_remember_a_hundred() {
        let mut paths = Vec::new();
        for n in 0..120 {
            move_to_front(&mut paths, Path::new(&format!("file-{n}")));
        }
        move_to_front(&mut paths, Path::new("file-50"));
        assert_eq!(paths.len(), REMEMBERED);
        assert_eq!(paths[0], Path::new("file-50"));
        assert_eq!(
            paths
                .iter()
                .filter(|path| *path == Path::new("file-50"))
                .count(),
            1
        );
        assert!(!paths.contains(&PathBuf::from("file-0")));
        assert!(!paths.contains(&PathBuf::from("file-19")));
        assert!(paths.contains(&PathBuf::from("file-20")));
    }

    #[test]
    fn removing_one_entry_lets_the_next_in_line_move_up() {
        let mut paths: Vec<PathBuf> = (0..30).map(|n| PathBuf::from(format!("f{n}"))).collect();
        remove(&mut paths, 1);
        assert_eq!(paths.len(), 29);
        assert_eq!(paths[1], Path::new("f2"));
        remove(&mut paths, 99);
        assert_eq!(paths.len(), 29);
        crate::settings::update(|s| *s = crate::settings::Settings::default());
    }
}
