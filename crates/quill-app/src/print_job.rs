//! Saving generated PDFs, and handing them to `lp` for Linux printing.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub enum Target {
    Save(PathBuf),
    Print { copies: u32 },
}

pub fn finish(target: Target, bytes: &[u8]) -> Result<(), String> {
    match target {
        Target::Save(path) => quill_formats::write_atomic(&path, bytes).map_err(|e| {
            format!(
                "Quill could not save \u{201c}{}\u{201d}.\n\n{e}",
                file_name(&path)
            )
        }),
        Target::Print { copies } => write_and_submit(bytes, copies, send_to_printer),
    }
}

pub fn temporary_path() -> PathBuf {
    static NUMBER: AtomicU64 = AtomicU64::new(0);
    let n = NUMBER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("quill-print-{}-{n}.pdf", std::process::id()))
}

/// The viewer may read asynchronously, so retain the PDF in the OS temporary
/// directory. A private directory prevents other users from reading its contents.
#[cfg(not(target_os = "macos"))]
pub fn preview_pdf(bytes: &[u8]) -> Result<(), String> {
    let directory = temporary_path().with_extension("preview");
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder
        .create(&directory)
        .map_err(|e| format!("Quill could not prepare system preview.\n\n{e}"))?;
    let path = directory.join("Preview.pdf");
    let result = quill_formats::write_atomic(&path, bytes).and_then(|_| {
        #[cfg(windows)]
        let status = std::process::Command::new("rundll32")
            .arg("url.dll,FileProtocolHandler")
            .arg(&path)
            .status();
        #[cfg(not(windows))]
        let status = std::process::Command::new("xdg-open").arg(&path).status();
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => Err(
                "Quill could not open a PDF viewer. Set a default PDF application and try again."
                    .into(),
            ),
            Err(e) => Err(format!("Quill could not open system preview.\n\n{e}")),
        }
    });
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&directory);
    }
    result
}

/// A new directory only this user can enter, named after the process and
/// a counter. It is created with `mkdir` and mode 0700, which fails if
/// anything already has the name (a symlink included), so another user
/// cannot plant a path for Quill to write through.
fn private_directory() -> std::io::Result<PathBuf> {
    let mut last = None;
    for _ in 0..100 {
        let directory = temporary_path().with_extension("print");
        let builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        match builder.create(&directory) {
            Ok(()) => return Ok(directory),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free name")))
}

/// Create `path` as a new file only this user can read. It fails rather
/// than open anything that is already there, and never follows a symlink.
fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Write the PDF into its own private directory and hand it to `submit`.
/// When the print service accepts the job nothing is left behind. When it
/// rejects the job, the directory stays so the PDF can be inspected or
/// printed by hand; a PDF that could not be written is removed.
fn write_and_submit(
    bytes: &[u8],
    copies: u32,
    submit: impl FnOnce(&Path, u32) -> Result<(), String>,
) -> Result<(), String> {
    use std::io::Write;
    let prepare = |e: std::io::Error| format!("Quill could not prepare the print file.\n\n{e}");
    let directory = private_directory().map_err(prepare)?;
    let path = directory.join("Print.pdf");
    let written = create_private_file(&path).and_then(|mut f| f.write_all(bytes));
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&directory);
        return Err(prepare(e));
    }
    let result = submit(&path, copies).map_err(|e| format!("Quill could not print.\n\n{e}"));
    if result.is_ok()
        && let Err(e) = std::fs::remove_dir_all(&directory)
    {
        eprintln!("quill: could not remove temporary print file: {e}");
    }
    result
}

/// Hand a PDF to CUPS. Only Linux prints this way: macOS and Windows use
/// their native print panels and never reach here.
fn send_to_printer(path: &Path, copies: u32) -> Result<(), String> {
    let output = std::process::Command::new("lp")
        .arg("-n")
        .arg(copies.to_string())
        .arg(path)
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("document")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_jobs_remove_their_temporary_pdf() {
        let mut seen = None;
        write_and_submit(b"%PDF-test", 2, |submitted, copies| {
            assert_eq!(std::fs::read(submitted).unwrap(), b"%PDF-test");
            assert_eq!(copies, 2);
            seen = Some(submitted.to_path_buf());
            Ok(())
        })
        .unwrap();
        let path = seen.unwrap();
        assert!(!path.exists());
        assert!(!path.parent().unwrap().exists());
    }

    #[cfg(unix)]
    #[test]
    fn rejected_jobs_keep_a_private_pdf() {
        use std::os::unix::fs::PermissionsExt;
        let mut seen = None;
        let error = write_and_submit(b"%PDF-test", 1, |path, _| {
            seen = Some(path.to_path_buf());
            Err("offline".into())
        })
        .unwrap_err();
        assert!(error.contains("offline"));
        let path = seen.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"%PDF-test");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn existing_paths_and_symlinks_are_never_written_through() {
        let scratch = private_directory().unwrap();
        let target = scratch.join("victim.txt");
        std::fs::write(&target, b"keep me").unwrap();
        let link = scratch.join("Print.pdf");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        // A file or link already at the path is refused, not truncated.
        assert!(create_private_file(&link).is_err());
        assert!(create_private_file(&target).is_err());
        let dangling = scratch.join("Dangling.pdf");
        std::os::unix::fs::symlink(scratch.join("missing"), &dangling).unwrap();
        assert!(create_private_file(&dangling).is_err());
        assert!(!scratch.join("missing").exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep me");
        // Each job gets a directory of its own.
        let first = private_directory().unwrap();
        let second = private_directory().unwrap();
        assert_ne!(first, second);
        for d in [scratch, first, second] {
            std::fs::remove_dir_all(d).unwrap();
        }
    }
}
