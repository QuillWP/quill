//! Saving a file so a crash or a full disk cannot leave half of it.

use super::*;

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
    let name = destination
        .file_name()
        .ok_or_else(|| format!("{} has no file name", destination.display()))?;
    let mut source_file = std::fs::File::open(source).map_err(|e| file_error("open", source, e))?;
    let keep = permissions_to_keep(destination, Some(source));
    let (tmp, file) = create_temp(destination, name, keep.is_some())?;
    let mut cleanup = TempCleanup {
        path: Some(tmp),
        file: Some(file),
    };
    let tmp = cleanup.path.as_deref().unwrap();
    let file = cleanup.file.as_mut().unwrap();
    std::io::copy(&mut source_file, file).map_err(|e| file_error("copy to", tmp, e))?;
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
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    let keep = permissions_to_keep(path, None);
    let (tmp, file) = create_temp(path, name, keep.is_some())?;
    let mut cleanup = TempCleanup {
        path: Some(tmp),
        file: Some(file),
    };
    let tmp = cleanup.path.as_deref().unwrap();
    let file = cleanup.file.as_mut().unwrap();
    file.write_all(bytes)
        .map_err(|e| file_error("write", tmp, e))?;
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
) -> Result<(PathBuf, std::fs::File), String> {
    for _ in 0..100 {
        let n = TEMP_NUMBER.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(".qwp-saving-{}-{n}", std::process::id()));
        let tmp = path.with_file_name(temp_name);
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
            Ok(file) => return Ok((tmp, file)),
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
    std::fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| file_error("sync folder", parent, e))
}

#[cfg(not(unix))]
pub(crate) fn sync_parent(_: &Path) -> Result<(), String> {
    Ok(())
}

pub(crate) struct TempCleanup {
    pub(crate) path: Option<PathBuf>,
    pub(crate) file: Option<std::fs::File>,
}

impl Drop for TempCleanup {
    fn drop(&mut self) {
        // In particular, close before removing on Windows.
        drop(self.file.take());
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}
