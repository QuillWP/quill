//! Saving a file so a crash or a full disk cannot leave half of it.

use super::*;

/// Where the app wants a save's temporary file made instead of beside the
/// document, set once at start. On the Mac this is the folder the system
/// gives for replacing a file: a document chosen in the Save panel may be
/// written in the App Sandbox, but nothing else may be made beside it.
static SAVE_FOLDER: std::sync::OnceLock<fn(&Path) -> Option<PathBuf>> = std::sync::OnceLock::new();

/// Make each save's temporary file in the folder `folder` returns for the
/// document, rather than beside it. When it returns None, or a folder on
/// another volume (where the final rename could not be atomic), the file is
/// made beside the document as before.
pub fn set_save_folder(folder: fn(&Path) -> Option<PathBuf>) {
    let _ = SAVE_FOLDER.set(folder);
}

/// Durably replace `path` with `bytes`. The temporary file is beside the
/// destination, so the final rename is atomic and cannot cross filesystems.
/// This is public so the app's settings, dictionaries and other user data can
/// use the same safe write as documents.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_atomic_with(path, bytes, replace_path)
}

/// Durably copy `source` over `destination` without exposing a partial copy.
/// This is used for document backups, which may be too large to read into
/// memory just to pass their bytes to [`write_atomic`].
pub fn copy_atomic(source: &Path, destination: &Path) -> Result<(), String> {
    let destination = &through_links(destination);
    let name = destination
        .file_name()
        .ok_or_else(|| format!("{} has no file name", destination.display()))?;
    let mut source_file = std::fs::File::open(source).map_err(|e| file_error("open", source, e))?;
    let keep = permissions_to_keep(destination, Some(source));
    let mut cleanup = create_temp(destination, name, keep.is_some())?;
    let tmp = cleanup.path.as_deref().unwrap();
    let file = cleanup.file.as_mut().unwrap();
    std::io::copy(&mut source_file, file).map_err(|e| file_error("copy to", tmp, e))?;
    keep_metadata(
        if destination.exists() {
            destination
        } else {
            source
        },
        tmp,
        file,
    );
    if let Some(keep) = keep {
        file.set_permissions(keep)
            .map_err(|e| file_error("set permissions of", tmp, e))?;
    }
    file.flush().map_err(|e| file_error("flush", tmp, e))?;
    file.sync_all().map_err(|e| file_error("sync", tmp, e))?;
    drop(cleanup.file.take());

    let tmp = cleanup.path.as_deref().unwrap();
    replace_path(tmp, destination).map_err(|e| {
        format!(
            "Could not replace \u{201c}{}\u{201d} with the completed backup: {e}",
            destination.display()
        )
    })?;
    cleanup.path = None;
    sync_parent(destination)?;
    Ok(())
}

pub(crate) fn write_atomic_with(
    path: &Path,
    bytes: &[u8],
    mut replace: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let path = &through_links(path);
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    let keep = permissions_to_keep(path, None);
    let mut cleanup = create_temp(path, name, keep.is_some())?;
    let tmp = cleanup.path.as_deref().unwrap();
    let file = cleanup.file.as_mut().unwrap();
    file.write_all(bytes)
        .map_err(|e| file_error("write", tmp, e))?;
    if path.exists() {
        keep_metadata(path, tmp, file);
    }
    if let Some(keep) = keep {
        file.set_permissions(keep)
            .map_err(|e| file_error("set permissions of", tmp, e))?;
    }
    file.flush().map_err(|e| file_error("flush", tmp, e))?;
    file.sync_all().map_err(|e| file_error("sync", tmp, e))?;
    // Windows cannot replace a destination while this handle remains open.
    drop(cleanup.file.take());

    let tmp = cleanup.path.as_deref().unwrap();
    replace(tmp, path).map_err(|e| {
        format!(
            "Could not replace \u{201c}{}\u{201d} with the completed temporary file: {e}",
            path.display()
        )
    })?;
    cleanup.path = None;
    sync_parent(path)?;
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn replace_path(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(windows)]
pub(crate) fn replace_path(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    retry_io(
        || {
            let ok = unsafe {
                MoveFileExW(
                    from.as_ptr(),
                    to.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if ok != 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        },
        transient_windows_replace_error,
        std::thread::sleep,
    )
}

/// Retry a bounded number of times with short exponential delays. Antivirus,
/// indexers and preview handlers commonly release Windows files within this
/// window; a persistent lock still returns the original OS error promptly.
#[cfg(any(windows, test))]
pub(crate) fn retry_io(
    mut operation: impl FnMut() -> std::io::Result<()>,
    retry: impl Fn(&std::io::Error) -> bool,
    mut pause: impl FnMut(std::time::Duration),
) -> std::io::Result<()> {
    const ATTEMPTS: usize = 6;
    for attempt in 0..ATTEMPTS {
        match operation() {
            Ok(()) => return Ok(()),
            Err(e) if attempt + 1 < ATTEMPTS && retry(&e) => {
                pause(std::time::Duration::from_millis(25 << attempt.min(4)));
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("the retry loop always returns on its final attempt")
}

#[cfg(any(windows, test))]
pub(crate) fn transient_windows_replace_error(error: &std::io::Error) -> bool {
    // Access denied, sharing violation, and lock violation. All other errors
    // (bad path, full disk, permissions) are immediately actionable.
    matches!(error.raw_os_error(), Some(5 | 32 | 33))
}

/// The file a save to `path` should replace. A symlink is followed to the
/// file it names, so the link stays a link and its target gets the new
/// contents, rather than the rename putting a plain file where the link
/// was. A chain of links that never ends (a loop) is left as it is.
pub(crate) fn through_links(path: &Path) -> PathBuf {
    let mut at = path.to_path_buf();
    // The same bound as the system's own (MAXSYMLINKS).
    for _ in 0..32 {
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => match std::fs::read_link(&at) {
                // A relative target is relative to the link's folder.
                Ok(target) => {
                    at = match at.parent() {
                        Some(folder) => folder.join(target),
                        None => target,
                    }
                }
                Err(_) => return path.to_path_buf(),
            },
            _ => return at,
        }
    }
    path.to_path_buf()
}

/// Give the temporary file what the file it replaces has besides its
/// contents and mode: its owner and group, and on the Mac its extended
/// attributes (Finder tags, comments, where it was downloaded from) and
/// access control list. The modification time is not copied, since the
/// file has changed. This is best effort: a file system without them, or
/// an owner this user may not give, leaves the save as it was.
#[allow(unused_variables)]
pub(crate) fn keep_metadata(from: &Path, tmp: &Path, file: &std::fs::File) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(m) = std::fs::metadata(from)
            && let Ok(own) = file.metadata()
            && (m.uid(), m.gid()) != (own.uid(), own.gid())
        {
            let _ = std::os::unix::fs::fchown(file, Some(m.uid()), Some(m.gid()));
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let (Ok(from), Ok(to)) = (
            std::ffi::CString::new(from.as_os_str().as_bytes()),
            std::ffi::CString::new(tmp.as_os_str().as_bytes()),
        ) else {
            return;
        };
        // SAFETY: both are valid NUL-terminated paths, and a null state is
        // allowed. Only attributes and the ACL are copied, never data.
        unsafe {
            libc::copyfile(
                from.as_ptr(),
                to.as_ptr(),
                std::ptr::null_mut(),
                libc::COPYFILE_XATTR | libc::COPYFILE_ACL,
            );
        }
    }
}

/// Create a unique temporary file without overwriting one left by a crashed
/// process or another window saving the same document.
///
/// With `private` the file is readable by its owner only until the caller
/// gives it its final permissions, so nobody else can read the contents of
/// a document that was private while it is being written.
pub(crate) fn create_temp(
    path: &Path,
    name: &std::ffi::OsStr,
    private: bool,
) -> Result<TempCleanup, String> {
    let folder = SAVE_FOLDER
        .get()
        .and_then(|folder| folder(path))
        .filter(|folder| same_volume(folder, path));
    let mut cleanup = TempCleanup {
        path: None,
        file: None,
        folder: folder.clone(),
    };
    for _ in 0..100 {
        let n = TEMP_NUMBER.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(".qwp-saving-{}-{n}", std::process::id()));
        let tmp = match &folder {
            Some(folder) => folder.join(temp_name),
            None => path.with_file_name(temp_name),
        };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = private;
        match options.open(&tmp) {
            Ok(file) => {
                cleanup.path = Some(tmp);
                cleanup.file = Some(file);
                return Ok(cleanup);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(file_error("create", &tmp, e)),
        }
    }
    Err(format!(
        "Could not create a unique temporary file beside \u{201c}{}\u{201d}",
        path.display()
    ))
}

/// The permissions the new copy of a file should have: those of the file it
/// replaces, else of `fallback`. Only on Unix, where a document may be
/// readable by its owner alone; other systems keep their defaults.
pub(crate) fn permissions_to_keep(
    destination: &Path,
    fallback: Option<&Path>,
) -> Option<std::fs::Permissions> {
    if !cfg!(unix) {
        return None;
    }
    std::fs::metadata(destination)
        .or_else(|e| fallback.map_or(Err(e), std::fs::metadata))
        .ok()
        .map(|m| m.permissions())
}

pub(crate) fn file_error(action: &str, path: &Path, error: std::io::Error) -> String {
    format!(
        "Could not {action} \u{201c}{}\u{201d}: {error}",
        path.display()
    )
}

#[cfg(unix)]
pub(crate) fn sync_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    match std::fs::File::open(parent) {
        Ok(dir) => dir.sync_all(),
        // The App Sandbox lets Quill write the document but not open its
        // folder. The file itself was synced before the rename.
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
        Err(e) => Err(e),
    }
    .map_err(|e| file_error("sync folder", parent, e))
}

/// True unless `folder` is known to be on another volume than `path` (the
/// file itself, or the folder it will be made in). When either cannot be
/// looked at, the folder is trusted, since the system chose it for `path`.
fn same_volume(folder: &Path, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let target = std::fs::metadata(path).or_else(|_| {
            std::fs::metadata(
                path.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )
        });
        if let (Ok(folder), Ok(target)) = (std::fs::metadata(folder), target) {
            return folder.dev() == target.dev();
        }
    }
    #[cfg(not(unix))]
    let _ = (folder, path);
    true
}

#[cfg(not(unix))]
pub(crate) fn sync_parent(_: &Path) -> Result<(), String> {
    Ok(())
}

pub(crate) struct TempCleanup {
    pub(crate) path: Option<PathBuf>,
    pub(crate) file: Option<std::fs::File>,
    /// The folder the app gave for this save, removed once it is empty.
    pub(crate) folder: Option<PathBuf>,
}

impl Drop for TempCleanup {
    fn drop(&mut self) {
        // In particular, close before removing on Windows.
        drop(self.file.take());
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
        if let Some(folder) = self.folder.take() {
            let _ = std::fs::remove_dir(folder);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("quill-atomic-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A save folder for documents in a "save-folder" scratch folder only,
    /// so the other tests here still save beside their files.
    fn test_save_folder(document: &Path) -> Option<PathBuf> {
        let scratch = document.parent()?;
        if !scratch
            .to_string_lossy()
            .contains("quill-atomic-save-folder")
        {
            return None;
        }
        let n = TEMP_NUMBER.fetch_add(1, Ordering::Relaxed);
        let folder = scratch.with_file_name(format!("{}-given-{n}", scratch.display()));
        std::fs::create_dir(&folder).ok()?;
        GIVEN.lock().unwrap().push(folder.clone());
        Some(folder)
    }

    static GIVEN: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

    #[test]
    fn a_save_folder_from_the_app_holds_the_temporary_file_and_is_removed() {
        set_save_folder(test_save_folder);
        let dir = scratch("save-folder");
        let given = |dir: &Path| {
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            std::fs::read_dir(dir.parent().unwrap())
                .unwrap()
                .filter(|e| {
                    e.as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(&format!("{name}-given-"))
                })
                .count()
        };
        let doc = dir.join("Report.qwp");
        // A new file, then replacing it, then a backup copy of it.
        write_atomic(&doc, b"first").unwrap();
        write_atomic(&doc, b"second").unwrap();
        let backup = dir.join("Backup of Report.qwp");
        copy_atomic(&doc, &backup).unwrap();
        assert_eq!(std::fs::read(&doc).unwrap(), b"second");
        assert_eq!(std::fs::read(&backup).unwrap(), b"second");
        // Each of the three asked for a folder of its own and used it.
        assert_eq!(GIVEN.lock().unwrap().len(), 3);
        // Nothing was left beside the documents or in the given folders,
        // and each given folder is gone.
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["Backup of Report.qwp", "Report.qwp"]);
        assert_eq!(given(&dir), 0);
        // A failed save removes its given folder too.
        std::fs::create_dir(dir.join("Folder.qwp")).unwrap();
        assert!(write_atomic(&dir.join("Folder.qwp"), b"x").is_err());
        assert_eq!(given(&dir), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_through_a_symlink_replaces_its_target_and_keeps_the_link() {
        let dir = scratch("link");
        std::fs::create_dir(dir.join("real")).unwrap();
        let target = dir.join("real/doc.qwp");
        std::fs::write(&target, b"old").unwrap();
        let link = dir.join("doc.qwp");
        std::os::unix::fs::symlink("real/doc.qwp", &link).unwrap();
        // A link to a link is followed to the end.
        let second = dir.join("again.qwp");
        std::os::unix::fs::symlink(&link, &second).unwrap();

        write_atomic(&second, b"new").unwrap();

        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            std::fs::symlink_metadata(&second)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        // No temporary file is left in either folder.
        for folder in [&dir, &dir.join("real")] {
            let names: Vec<_> = std::fs::read_dir(folder)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert!(
                names
                    .iter()
                    .all(|n| !n.to_string_lossy().contains("qwp-saving")),
                "{names:?}"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_link_loop_or_a_dangling_link_still_saves() {
        let dir = scratch("loop");
        let a = dir.join("a.qwp");
        let b = dir.join("b.qwp");
        std::os::unix::fs::symlink(&b, &a).unwrap();
        std::os::unix::fs::symlink(&a, &b).unwrap();
        assert_eq!(through_links(&a), a);
        write_atomic(&a, b"x").unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"x");

        // A link to a file not made yet makes that file.
        let dangling = dir.join("c.qwp");
        std::os::unix::fs::symlink("made.qwp", &dangling).unwrap();
        write_atomic(&dangling, b"y").unwrap();
        assert_eq!(std::fs::read(dir.join("made.qwp")).unwrap(), b"y");
        assert!(
            std::fs::symlink_metadata(&dangling)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_save_keeps_the_files_extended_attributes() {
        use std::os::unix::ffi::OsStrExt;
        let dir = scratch("xattr");
        let path = dir.join("doc.qwp");
        std::fs::write(&path, b"old").unwrap();
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let name = c"com.quill.test";
        let value = b"kept";
        // SAFETY: valid NUL-terminated strings and a buffer of the length given.
        let set = unsafe {
            libc::setxattr(
                c_path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        assert_eq!(set, 0, "the scratch folder takes extended attributes");

        write_atomic(&path, b"new").unwrap();

        let mut buf = [0u8; 16];
        // SAFETY: as above, with a buffer of `buf.len()` bytes.
        let n = unsafe {
            libc::getxattr(
                c_path.as_ptr(),
                name.as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                0,
                0,
            )
        };
        assert_eq!(&buf[..n.max(0) as usize], value);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
