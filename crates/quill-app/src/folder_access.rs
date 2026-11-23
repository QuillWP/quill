//! Asking once to be let into a document's folder. In the App Sandbox Quill
//! may use only the files the person chose, and a Markdown file keeps its
//! pictures in a folder beside it. When the sandbox refuses those, Quill asks
//! for the folder in an Open panel, remembers the answer in the settings and
//! keeps a bookmark, so it never asks about that folder again.

use std::path::Path;

/// True when this copy of Quill runs in the App Sandbox. The system sets
/// this variable for every sandboxed process.
pub fn sandboxed() -> bool {
    std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some()
}

/// After the sandbox refused something beside `document`: unless Quill can
/// already look in the document's folder, ask the person to allow it, saying
/// Quill needs it to `purpose` (for example "save the pictures of"). True
/// when the folder is allowed now and the caller should try again. Outside
/// the sandbox, or when the folder was already allowed, nothing is asked.
pub fn ask_for_folder_of(document: &Path, purpose: &str) -> bool {
    if !sandboxed() {
        return false;
    }
    let Some(folder) = document.parent().filter(|f| !f.as_os_str().is_empty()) else {
        return false;
    };
    if std::fs::read_dir(folder).is_ok() {
        // Allowed already, so the refusal was about something else.
        return false;
    }
    let name = document
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let message = format!(
        "Quill needs your permission to {purpose} \u{201c}{name}\u{201d} in this folder. \
         Click Allow to let it use the folder from now on."
    );
    #[cfg(target_os = "macos")]
    let chosen = crate::mac_dialog::allow_folder(folder, &message);
    #[cfg(not(target_os = "macos"))]
    let chosen: Option<std::path::PathBuf> = {
        let _ = message;
        None
    };
    let Some(chosen) = chosen else {
        return false;
    };
    // The folder itself, or one that holds it, will do.
    let holds = |chosen: &Path| match (folder.canonicalize(), chosen.canonicalize()) {
        (Ok(folder), Ok(chosen)) => folder.starts_with(chosen),
        _ => folder.starts_with(chosen),
    };
    if !holds(&chosen) {
        crate::app::warn(
            &format!("That folder does not hold \u{201c}{name}\u{201d}."),
            "Quill asks only for the folder the document is in, or one that holds it.",
        );
        return false;
    }
    crate::settings::update(|s| {
        s.allowed_folders.retain(|f| f != &chosen);
        s.allowed_folders.insert(0, chosen.clone());
        s.allowed_folders
            .truncate(crate::settings::MAX_ALLOWED_FOLDERS);
    });
    crate::bookmarks::remember(&chosen);
    std::fs::read_dir(folder).is_ok()
}
