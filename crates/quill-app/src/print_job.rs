//! Saving generated PDFs, and handing them to `lp` for Linux printing.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub enum Target {
    Save(PathBuf),
    Print { path: PathBuf, copies: u32 },
}

pub fn finish(target: Target, bytes: &[u8]) -> Result<(), String> {
    match target {
        Target::Save(path) => quill_formats::write_atomic(&path, bytes).map_err(|e| {
            format!(
                "Quill could not save \u{201c}{}\u{201d}.\n\n{e}",
                file_name(&path)
            )
        }),
        Target::Print { path, copies } => write_and_submit(&path, bytes, copies, send_to_printer),
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

fn write_and_submit(
    path: &Path,
    bytes: &[u8],
    copies: u32,
    submit: impl FnOnce(&Path, u32) -> Result<(), String>,
) -> Result<(), String> {
    std::fs::write(path, bytes)
        .map_err(|e| format!("Quill could not prepare the print file.\n\n{e}"))?;
    let result = submit(path, copies).map_err(|e| format!("Quill could not print.\n\n{e}"));
    // Once the print service has accepted the job it no longer needs
    // Quill's source file. Keep a rejected job for diagnosis/retry.
    if result.is_ok()
        && let Err(e) = std::fs::remove_file(path)
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
        let path = temporary_path();
        write_and_submit(&path, b"%PDF-test", 2, |submitted, copies| {
            assert_eq!(std::fs::read(submitted).unwrap(), b"%PDF-test");
            assert_eq!(copies, 2);
            Ok(())
        })
        .unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn rejected_jobs_keep_their_temporary_pdf() {
        let path = temporary_path();
        let error =
            write_and_submit(&path, b"%PDF-test", 1, |_, _| Err("offline".into())).unwrap_err();
        assert!(error.contains("offline"));
        assert_eq!(std::fs::read(&path).unwrap(), b"%PDF-test");
        std::fs::remove_file(path).unwrap();
    }
}
