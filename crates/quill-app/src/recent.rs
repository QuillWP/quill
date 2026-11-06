//! Configurable recent documents, persisted by Quill on every platform.
//! macOS also updates NSDocumentController for system recent-document support.

use std::path::{Path, PathBuf};

fn limit() -> usize {
    crate::settings::with(|s| s.recent_count.clamp(1, 50))
}

fn move_to_front(paths: &mut Vec<PathBuf>, path: &Path) {
    paths.retain(|old| old != path);
    paths.insert(0, path.to_path_buf());
    paths.truncate(limit());
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
        paths = (0..urls.count().min(limit()))
            .filter_map(|i| urls.objectAtIndex(i).path())
            .map(|p| PathBuf::from(p.to_string()))
            .collect();
    }
    paths.truncate(limit());
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
    fn recent_paths_are_unique_newest_first_and_bounded() {
        let mut paths = Vec::new();
        for n in 0..12 {
            move_to_front(&mut paths, Path::new(&format!("file-{n}")));
        }
        move_to_front(&mut paths, Path::new("file-5"));
        assert_eq!(paths.len(), limit());
        assert_eq!(paths[0], Path::new("file-5"));
        assert_eq!(
            paths
                .iter()
                .filter(|path| *path == Path::new("file-5"))
                .count(),
            1
        );
        assert!(!paths.contains(&PathBuf::from("file-0")));
        let original = crate::settings::get();
        crate::settings::update(|s| s.recent_count = 25);
        for n in 0..60 {
            move_to_front(&mut paths, Path::new(&format!("many-{n}")));
        }
        assert_eq!(paths.len(), 25);
        assert_eq!(paths[0], Path::new("many-59"));
        crate::settings::update(|s| *s = original);
    }
}
