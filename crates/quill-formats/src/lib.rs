//! Reading and writing Quill documents.

mod comments;
pub mod docx;
mod export;
pub mod html;
mod html_read;
pub mod images;
mod math;
pub mod native;
mod numfmt;
pub mod odt;
pub mod odt_read;
pub mod png;
pub mod report;
pub mod rtf;
pub mod xml;
pub mod zip;

use quill_core::Document;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_NUMBER: AtomicU64 = AtomicU64::new(0);

/// Largest document Quill will read into memory. File formats may impose
/// tighter limits on individual parts after the container is opened.
// Room for Quill's own RTF of the largest document it holds, whose text
// may be written as escapes several bytes long.
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// File formats Quill can save, in the order Save As offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Quill's own format, and the default: keeps everything exactly.
    Quill,
    /// Word's format today, for exchanging documents.
    Docx,
    /// Interchange Format (RTF).
    Rtf,
    /// OpenDocument Text export.
    Odt,
    /// A self-contained HTML document export.
    Html,
    /// Text Only.
    Text,
}

impl Format {
    pub const ALL: [Format; 6] = [
        Format::Quill,
        Format::Docx,
        Format::Rtf,
        Format::Odt,
        Format::Html,
        Format::Text,
    ];

    /// Saving in these formats exports a copy and leaves the document as it
    /// was: Quill's ODT and HTML writers keep less than a .qwp file does.
    pub fn export_only(self) -> bool {
        matches!(self, Format::Odt | Format::Html)
    }

    /// The name the Save As dialog shows.
    pub fn name(self) -> &'static str {
        match self {
            Format::Quill => "Quill Document",
            Format::Docx => "Word Document",
            Format::Rtf => "Interchange Format (RTF)",
            Format::Odt => "OpenDocument Text (Export)",
            Format::Html => "HTML Document (Export)",
            Format::Text => "Text Only",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Format::Quill => &["qwp"],
            Format::Docx => &["docx"],
            Format::Rtf => &["rtf"],
            Format::Odt => &["odt"],
            Format::Html => &["html", "htm"],
            Format::Text => &["txt", "text"],
        }
    }

    /// The format a file name implies, if any.
    pub fn from_path(path: &Path) -> Option<Format> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Format::ALL
            .into_iter()
            .find(|f| f.extensions().contains(&ext.as_str()))
    }
}

/// Every extension Open can read.
pub const READABLE: &[&str] = &[
    "qwp", "docx", "docm", "dotx", "dotm", "rtf", "odt", "ott", "odm", "fodt", "html", "htm",
    "txt", "text",
];

/// Open a file, deciding the format from its contents first and its name
/// second, the way Word's "Readable Files" does.
pub fn load(path: &Path) -> Result<Document, String> {
    load_with(path, true)
}

/// Like `load`, but with `interpret_rtf` false RTF opens as its plain
/// codes, as Word does with Always Interpret RTF turned off.
pub fn load_with(path: &Path, interpret_rtf: bool) -> Result<Document, String> {
    load_reported(path, interpret_rtf).map(|(doc, _)| doc)
}

/// Like `load_with`, with what the reader could not keep exactly (empty
/// for Quill's own files and plain text).
pub fn load_reported(
    path: &Path,
    interpret_rtf: bool,
) -> Result<(Document, report::ImportReport), String> {
    // Whatever the file says, its measurements end up within Word's range,
    // and it's no bigger than Quill can save and open again.
    let (mut doc, report) = read_any(path, interpret_rtf)?;
    too_large(path, doc.size())?;
    doc.keep_in_bounds();
    Ok((doc, report))
}

/// Open a file as plain text, whatever its name says: for a file named as
/// a document that holds text instead (`holds_text_not_its_format`).
pub fn load_as_text(path: &Path) -> Result<Document, String> {
    let bytes = read_file_with_limit(path, MAX_FILE_BYTES)?;
    plain_text(path, bytes)
}

/// The start of an old Word 97-2003 (.doc) file, and of other Office
/// files of that time.
const OLD_OFFICE: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

/// A password-protected .docx is not a zip: Office wraps the encrypted
/// package in the same container as an old .doc. The container's table of
/// contents names the package `EncryptedPackage`, in UTF-16.
fn has_password(bytes: &[u8]) -> bool {
    let name: Vec<u8> = "EncryptedPackage"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    bytes.windows(name.len()).any(|w| w == name)
}

/// Names that promise a zip package: Word's, OpenDocument's and Quill's.
fn named_as_package(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "docx" | "docm" | "dotx" | "dotm" => Some("Word"),
        "odt" | "ott" | "odm" => Some("OpenDocument"),
        "qwp" => Some("Quill"),
        _ => None,
    }
}

/// Text rather than some other kind of file: no zero bytes, and hardly
/// any control characters besides tabs, line ends and page breaks.
fn looks_like_text(bytes: &[u8]) -> bool {
    if utf16(bytes).is_some() {
        return true;
    }
    let head = &bytes[..bytes.len().min(8192)];
    let control = head
        .iter()
        .filter(|&&b| b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0C))
        .count();
    !head.contains(&0) && control * 100 <= head.len()
}

/// Text in UTF-16 with its byte-order mark, as Windows Notepad saves
/// "Unicode" text.
fn utf16(bytes: &[u8]) -> Option<String> {
    let big = match bytes.get(..2)? {
        [0xFF, 0xFE] => false,
        [0xFE, 0xFF] => true,
        _ => return None,
    };
    let units: Vec<u16> = bytes[2..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| {
            if big {
                u16::from_be_bytes(b)
            } else {
                u16::from_le_bytes(b)
            }
        })
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// Whether `path` has a document's name (.docx, .odt, .qwp) but holds
/// plain text, so Open can offer to show it as text.
pub fn holds_text_not_its_format(path: &Path) -> bool {
    let Some(_) = named_as_package(path) else {
        return false;
    };
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::new();
    if file.take(8192).read_to_end(&mut head).is_err() {
        return false;
    }
    !head.starts_with(b"PK\x03\x04") && !head.is_empty() && looks_like_text(&head)
}

fn too_large(path: &Path, size: quill_core::limits::Size) -> Result<(), String> {
    match size.too_large() {
        Some(why) => Err(format!(
            "\u{201c}{}\u{201d} is too large for Quill: {why}.",
            path.display()
        )),
        None => Ok(()),
    }
}

/// A file read as plain text, checked for size before any paragraphs are
/// made, since a huge file of short lines would otherwise fill memory.
fn plain_text(path: &Path, bytes: Vec<u8>) -> Result<Document, String> {
    let text = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) if let Some(text) = utf16(e.as_bytes()) => text,
        // Old files are often Mac Roman or Latin-1; Latin-1 keeps every
        // byte readable even when a few accents come out wrong.
        Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
    };
    too_large(path, quill_core::limits::Size::of_text(&text))?;
    Ok(quill_core::text::from_plain_text(&text))
}

fn read_any(path: &Path, interpret_rtf: bool) -> Result<(Document, report::ImportReport), String> {
    let plain = |doc: Document| (doc, report::ImportReport::default());
    let bytes = read_file_with_limit(path, MAX_FILE_BYTES)?;
    let name = || {
        path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    if bytes.starts_with(OLD_OFFICE) {
        if has_password(&bytes) {
            return Err(format!(
                "\u{201c}{}\u{201d} is protected by a password, which Quill can't open. \
                 Remove the password in Word or LibreOffice and save it again.",
                name()
            ));
        }
        return Err(format!(
            "\u{201c}{}\u{201d} is an old Word 97-2003 document (.doc), which Quill can't open. \
             Open it in Word or LibreOffice and save it as .docx.",
            name()
        ));
    }
    if !bytes.starts_with(b"PK\x03\x04")
        && let Some(kind) = named_as_package(path)
    {
        return Err(if looks_like_text(&bytes) {
            format!(
                "\u{201c}{}\u{201d} isn't a {kind} document: it holds plain text.",
                name()
            )
        } else {
            format!("\u{201c}{}\u{201d} isn't a {kind} document.", name())
        });
    }
    if bytes.starts_with(b"PK\x03\x04") {
        if native::is_quill(&bytes) {
            return native::read(&bytes)
                .map(plain)
                .map_err(|e| document_error(path, "Quill", e));
        }
        if odt::is_odt(&bytes) {
            return odt_read::read_reported(&bytes).map_err(|e| document_error(path, "ODT", e));
        }
        return docx::read::read_reported(&bytes).map_err(|e| {
            // A zip with no Word document in it is some other archive.
            if e.contains("no document part") {
                format!(
                    "\u{201c}{}\u{201d} is a zip archive, not a document Quill can open.",
                    name()
                )
            } else {
                document_error(path, "DOCX", e)
            }
        });
    }
    if interpret_rtf && bytes.starts_with(b"{\\rtf") {
        return rtf::read::read_reported(&bytes).map_err(|e| document_error(path, "RTF", e));
    }
    if odt_read::is_flat(&bytes) {
        return odt_read::read_flat_reported(&bytes).map_err(|e| document_error(path, "ODT", e));
    }
    if html_read::is_html(&bytes) || Format::from_path(path) == Some(Format::Html) {
        return html_read::read_file(&bytes).map_err(|e| document_error(path, "HTML", e));
    }
    if Format::from_path(path) == Some(Format::Odt) {
        return Err(format!(
            "\u{201c}{}\u{201d} is not an OpenDocument Text file Quill can read.",
            path.display()
        ));
    }
    // Anything else is read as text, if it is text.
    if !looks_like_text(&bytes) {
        return Err(format!(
            "\u{201c}{}\u{201d} isn't a document Quill can read.",
            name()
        ));
    }
    plain_text(path, bytes).map(plain)
}

/// How far superscript text of `size` half-points is raised: a third of
/// its size, in whole half-points, the unit .docx and RTF measure it in,
/// so the shift comes back the same after saving.
pub(crate) fn superscript_rise(size: u16) -> i32 {
    (size as i32 + 1) / 3 * 10
}

/// How far subscript text of `size` half-points is lowered: a sixth of its
/// size, in whole half-points.
pub(crate) fn subscript_drop(size: u16) -> i32 {
    (size as i32 + 3) / 6 * 10
}

/// A baseline shift in twips as the nearest whole half-point.
pub(crate) fn half_points(twips: i32) -> i32 {
    (twips + 5 * twips.signum()) / 10
}

fn document_error(path: &Path, feature: &str, error: String) -> String {
    format!(
        "Could not read the {feature} document data in \u{201c}{}\u{201d}: {error}",
        path.display()
    )
}

/// Read at most `limit` bytes, checking both the initial metadata and the
/// bytes actually read in case the file grows while it is being opened.
fn read_file_with_limit(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("Could not open \u{201c}{}\u{201d}: {e}", path.display()))?;
    let len = file
        .metadata()
        .map_err(|e| format!("Could not inspect \u{201c}{}\u{201d}: {e}", path.display()))?
        .len();
    if len > limit {
        return Err(format!(
            "\u{201c}{}\u{201d} is too large for Quill to open ({} MB maximum).",
            path.display(),
            limit / (1024 * 1024)
        ));
    }
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Could not read \u{201c}{}\u{201d}: {e}", path.display()))?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "\u{201c}{}\u{201d} is too large for Quill to open ({} MB maximum).",
            path.display(),
            limit / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

/// Save a document. Writes a temporary file next to the target and
/// renames it into place, so a failed save never damages the old file.
pub fn save(doc: &Document, path: &Path, format: Format) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    if let Some(why) = doc.size().too_large() {
        return Err(format!("Quill didn't save \u{201c}{file}\u{201d}: {why}."));
    }
    // The exports and plain text carry no tracked changes: they get the
    // text with every change accepted.
    let accepted;
    let doc = if matches!(format, Format::Odt | Format::Html | Format::Text) && doc.has_changes() {
        accepted = {
            let mut d = doc.compact_for_save();
            d.resolve_all_changes(true);
            d
        };
        &accepted
    } else {
        doc
    };
    let bytes = match format {
        Format::Quill => native::write(doc, now),
        Format::Docx => docx::write(doc, now),
        Format::Rtf => rtf::write(doc),
        Format::Odt => odt::write(doc, now),
        Format::Html => html::write(doc),
        Format::Text => {
            let eol = if cfg!(windows) { "\r\n" } else { "\n" };
            quill_core::text::to_plain_text(doc, eol).into_bytes()
        }
    };
    // Read the result back before it replaces anything, so a save that
    // couldn't be opened again never reaches the disk.
    let back = match format {
        Format::Quill => native::read(&bytes).map(drop),
        Format::Docx => docx::read(&bytes).map(drop),
        Format::Rtf => rtf::read(&bytes).map(drop),
        Format::Odt => odt_read::read(&bytes).map(drop),
        Format::Html => html_read::read_file(&bytes).map(drop),
        Format::Text => Ok(()),
    };
    if let Err(e) = back {
        return Err(format!(
            "Quill didn't save \u{201c}{file}\u{201d}, because it couldn't open the result again \
             ({e}). The file on disk is unchanged.{}",
            if format == Format::Quill {
                ""
            } else {
                " Saving in Quill's own format may work."
            }
        ));
    }
    write_atomic(path, &bytes)
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
    let name = destination
        .file_name()
        .ok_or_else(|| format!("{} has no file name", destination.display()))?;
    let mut source_file = std::fs::File::open(source).map_err(|e| file_error("open", source, e))?;
    let (tmp, file) = create_temp(destination, name)?;
    let mut cleanup = TempCleanup {
        path: Some(tmp),
        file: Some(file),
    };
    let tmp = cleanup.path.as_deref().unwrap();
    let file = cleanup.file.as_mut().unwrap();
    std::io::copy(&mut source_file, file).map_err(|e| file_error("copy to", tmp, e))?;
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

fn write_atomic_with(
    path: &Path,
    bytes: &[u8],
    mut replace: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    let (tmp, file) = create_temp(path, name)?;
    let mut cleanup = TempCleanup {
        path: Some(tmp),
        file: Some(file),
    };
    let tmp = cleanup.path.as_deref().unwrap();
    let file = cleanup.file.as_mut().unwrap();
    file.write_all(bytes)
        .map_err(|e| file_error("write", tmp, e))?;
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
fn replace_path(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(windows)]
fn replace_path(from: &Path, to: &Path) -> std::io::Result<()> {
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
fn retry_io(
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
fn transient_windows_replace_error(error: &std::io::Error) -> bool {
    // Access denied, sharing violation, and lock violation. All other errors
    // (bad path, full disk, permissions) are immediately actionable.
    matches!(error.raw_os_error(), Some(5 | 32 | 33))
}

/// Create a unique temporary file without overwriting one left by a crashed
/// process or another window saving the same document.
fn create_temp(path: &Path, name: &std::ffi::OsStr) -> Result<(PathBuf, std::fs::File), String> {
    for _ in 0..100 {
        let n = TEMP_NUMBER.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(".qwp-saving-{}-{n}", std::process::id()));
        let tmp = path.with_file_name(temp_name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
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

fn file_error(action: &str, path: &Path, error: std::io::Error) -> String {
    format!(
        "Could not {action} \u{201c}{}\u{201d}: {error}",
        path.display()
    )
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| file_error("sync folder", parent, e))
}

#[cfg(not(unix))]
fn sync_parent(_: &Path) -> Result<(), String> {
    Ok(())
}

struct TempCleanup {
    path: Option<PathBuf>,
    file: Option<std::fs::File>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_that_are_not_what_their_names_say_get_a_clear_answer() {
        let dir = std::env::temp_dir().join(format!("quill-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, bytes: &[u8]| {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let error = |path: &Path| load(path).err().unwrap_or_default();

        // Text with a Word document's name: refused, but it can open as text.
        let fake = file("notes.docx", b"Just some text\r\non two lines\r\n");
        assert!(error(&fake).contains("isn't a Word document: it holds plain text"));
        assert!(holds_text_not_its_format(&fake));
        let doc = load_as_text(&fake).unwrap();
        assert_eq!(doc.main.paras[0].text, "Just some text");

        // An old .doc, whatever its name.
        let mut old = OLD_OFFICE.to_vec();
        old.extend([0; 600]);
        assert!(error(&file("report.docx", &old)).contains("Word 97-2003"));
        assert!(error(&file("report.txt", &old)).contains("Word 97-2003"));

        // A .docx with a password uses the same container.
        old.extend("EncryptedPackage".encode_utf16().flat_map(u16::to_le_bytes));
        let locked = error(&file("locked.docx", &old));
        assert!(locked.contains("protected by a password"), "{locked}");

        // A zip that isn't a document, and a file that isn't text.
        let mut zip = crate::zip::Writer::new(0);
        zip.add("readme.txt", b"hello");
        assert!(error(&file("bundle.zip", &zip.finish())).contains("zip archive"));
        assert!(
            error(&file("photo.txt", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"))
                .contains("isn't a document")
        );
        assert!(!holds_text_not_its_format(&file("real.txt", b"text")));

        // Windows Notepad's Unicode text.
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend("Caf\u{e9}".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(
            load(&file("notepad.txt", &utf16)).unwrap().main.paras[0].text,
            "Caf\u{e9}"
        );

        // Text with more paragraphs than Quill holds is refused before it is
        // turned into paragraphs.
        let lines = "\n".repeat(quill_core::limits::MAX_PARAGRAPHS + 1);
        assert!(error(&file("lines.txt", lines.as_bytes())).contains("too large for Quill"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn documents_over_the_size_limit_are_not_saved() {
        let mut doc = quill_core::text::from_plain_text("x");
        let p = doc.main.paras[0].clone();
        doc.main.paras = vec![p; quill_core::limits::MAX_PARAGRAPHS + 1];
        let path = std::env::temp_dir().join(format!("quill-over-{}.qwp", std::process::id()));
        let e = save(&doc, &path, Format::Quill).unwrap_err();
        assert!(e.contains("Quill's limit is"), "{e}");
        assert!(!path.exists());
    }

    /// Slow and needs over 2 GB of memory, so it runs only when asked:
    /// cargo test --release -p quill-formats -- --ignored at_the_size_limit
    #[test]
    #[ignore]
    fn documents_at_the_size_limit_save_and_open_again() {
        use quill_core::{CharProps, Paragraph, Rgb};
        let mut doc = Document::new();
        let font = doc.font_id("Helvetica Neue");
        let pap = doc.main.paras[0].pap.clone();
        let looks = [
            CharProps {
                bold: true,
                size: 28,
                ..Default::default()
            },
            CharProps {
                italic: true,
                color: Rgb(0xC00000),
                font,
                ..Default::default()
            },
            CharProps {
                underline: quill_core::Underline::Single,
                spacing: 20,
                position: 60,
                ..Default::default()
            },
            CharProps::default(),
        ];
        // Four runs of 32 characters a paragraph, many of them escapes.
        let piece: String = "A <tag> & \u{e9}\u{1F680} words.."
            .chars()
            .cycle()
            .take(32)
            .collect();
        let headers = doc.size().paragraphs - 1;
        doc.main.paras = (0..quill_core::limits::MAX_PARAGRAPHS - headers)
            .map(|_| {
                let mut p = Paragraph::new(pap.clone(), CharProps::default());
                for chp in looks {
                    let at = p.len();
                    p.insert(at, &piece, chp);
                }
                p
            })
            .collect();
        assert!(doc.size().too_large().is_none());
        for format in [Format::Quill, Format::Docx, Format::Rtf] {
            let path = std::env::temp_dir().join(format!(
                "quill-limit-{}.{}",
                std::process::id(),
                format.extensions()[0]
            ));
            save(&doc, &path, format).unwrap();
            let back = load(&path).unwrap();
            assert_eq!(back.size(), doc.size(), "{format:?}");
            let _ = std::fs::remove_file(&path);
        }
    }
    use quill_core::{Position, StoryId, text};

    #[test]
    fn save_and_load_files() {
        let dir = std::env::temp_dir().join(format!("quill-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = text::from_plain_text("One\nTwo");
        for (name, fmt) in [
            ("a.qwp", Format::Quill),
            ("a.docx", Format::Docx),
            ("a.rtf", Format::Rtf),
            ("a.txt", Format::Text),
        ] {
            let path = dir.join(name);
            assert_eq!(Format::from_path(&path), Some(fmt));
            save(&doc, &path, fmt).unwrap();
            let back = load(&path).unwrap();
            let texts: Vec<_> = back.main.paras.iter().map(|p| p.text.as_str()).collect();
            assert_eq!(texts, ["One", "Two"], "{name}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bounded_file_read_refuses_oversized_input() {
        let dir = std::env::temp_dir().join(format!("quill-read-limit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("large.txt");
        std::fs::write(&path, b"12345").unwrap();

        let error = read_file_with_limit(&path, 4).unwrap_err();

        assert!(error.contains("too large"), "{error}");
        assert!(error.contains("large.txt"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn damaged_document_errors_name_the_file_and_format() {
        let dir = std::env::temp_dir().join(format!("quill-error-context-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("damaged.rtf");
        std::fs::write(&path, b"{\\rtf1 \\'zz}").unwrap();

        let error = load(&path).unwrap_err();

        assert!(error.contains("damaged.rtf"), "{error}");
        assert!(error.contains("RTF"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_replaces_an_existing_file() {
        let dir = std::env::temp_dir().join(format!("quill-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("document.qwp");
        std::fs::write(&path, b"old").unwrap();

        write_atomic(&path, b"new").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failed_atomic_replace_keeps_the_old_file_and_removes_the_temporary_one() {
        let dir = std::env::temp_dir().join(format!("quill-atomic-fail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("document.qwp");
        std::fs::write(&path, b"old").unwrap();

        let error = write_atomic_with(&path, b"new", |_, _| {
            Err(std::io::Error::other("pretend replacement failed"))
        })
        .unwrap_err();

        assert!(error.contains("pretend replacement failed"));
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn transient_windows_replacement_errors_are_retried_with_a_bound() {
        let mut calls = 0;
        let mut waits = Vec::new();
        retry_io(
            || {
                calls += 1;
                if calls < 3 {
                    Err(std::io::Error::from_raw_os_error(32))
                } else {
                    Ok(())
                }
            },
            transient_windows_replace_error,
            |delay| waits.push(delay),
        )
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(
            waits,
            [
                std::time::Duration::from_millis(25),
                std::time::Duration::from_millis(50),
            ]
        );
    }

    #[test]
    fn permanent_replacement_errors_are_not_retried() {
        let mut calls = 0;
        let error = retry_io(
            || {
                calls += 1;
                Err(std::io::Error::from_raw_os_error(3))
            },
            transient_windows_replace_error,
            |_| panic!("a permanent error must not sleep"),
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(error.raw_os_error(), Some(3));
    }

    #[test]
    fn transient_replacement_retries_stop_after_six_attempts() {
        let mut calls = 0;
        let mut waits = 0;
        let error = retry_io(
            || {
                calls += 1;
                Err(std::io::Error::from_raw_os_error(33))
            },
            transient_windows_replace_error,
            |_| waits += 1,
        )
        .unwrap_err();
        assert_eq!((calls, waits), (6, 5));
        assert_eq!(error.raw_os_error(), Some(33));
    }

    #[test]
    fn atomic_copy_replaces_an_existing_backup() {
        let dir = std::env::temp_dir().join(format!("quill-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("document.qwp");
        let backup = dir.join("Backup of document.qwp");
        std::fs::write(&source, b"current document").unwrap();
        std::fs::write(&backup, b"older backup").unwrap();

        copy_atomic(&source, &backup).unwrap();

        assert_eq!(std::fs::read(&backup).unwrap(), b"current document");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_copy_replaces_a_read_only_backup_when_the_folder_allows_it() {
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("quill-backup-read-only-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("document.qwp");
        let backup = dir.join("Backup of document.qwp");
        std::fs::write(&source, b"current document").unwrap();
        std::fs::write(&backup, b"older backup").unwrap();
        std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o444)).unwrap();

        copy_atomic(&source, &backup).unwrap();

        assert_eq!(std::fs::read(&backup).unwrap(), b"current document");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failed_save_after_backup_preserves_both_saved_versions() {
        let dir =
            std::env::temp_dir().join(format!("quill-backup-save-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("document.qwp");
        let backup = dir.join("Backup of document.qwp");
        std::fs::write(&path, b"last saved version").unwrap();
        copy_atomic(&path, &backup).unwrap();

        write_atomic_with(&path, b"new version", |_, _| {
            Err(std::io::Error::other("pretend save failed"))
        })
        .unwrap_err();

        assert_eq!(std::fs::read(&path).unwrap(), b"last saved version");
        assert_eq!(std::fs::read(&backup).unwrap(), b"last saved version");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rtf_round_trip() {
        let mut doc = text::from_plain_text("Title\nCafé {braces} \\ back\tslash\nThird");
        let h1 = doc.styles.find("heading 1").unwrap();
        doc.apply_style(StoryId::Main, 0, 0, h1);
        doc.format_chars(
            StoryId::Main,
            Position::new(1, 0),
            Position::new(1, 5),
            |c| {
                c.bold = true;
                c.color = quill_core::Rgb(0xFF0000);
            },
        );
        doc.format_paras(StoryId::Main, 2, 2, |p| {
            p.align = quill_core::Align::Right;
            p.set_tab(quill_core::TabStop {
                pos: 2880,
                align: quill_core::TabAlign::Decimal,
                ..Default::default()
            });
        });
        doc.summary.author = "Lucas".into();
        let back = super::rtf::read(&super::rtf::write(&doc)).unwrap();
        let texts: Vec<_> = back.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["Title", "Café {braces} \\ back\tslash", "Third"]);
        assert_eq!(
            back.styles.get(back.main.paras[0].pap.style).name,
            "heading 1"
        );
        let c = back.main.paras[1].chp_of_char(0);
        assert!(c.bold);
        assert_eq!(c.color, quill_core::Rgb(0xFF0000));
        assert!(!back.main.paras[1].chp_of_char(6).bold);
        assert_eq!(back.main.paras[2].pap.align, quill_core::Align::Right);
        assert_eq!(back.main.paras[2].pap.tabs.len(), 1);
        assert_eq!(back.summary.author, "Lucas");
        assert_eq!(
            back.font_name(back.main.paras[0].chp_of_char(0).font),
            "Helvetica"
        );
    }

    #[test]
    fn reads_original_style_mac_rtf() {
        // The shape of RTF the original wrote: \mac character set,
        // fonts listed in one group, Mac Roman bytes.
        let rtf = br"{\rtf1\mac\deff2 {\fonttbl{\f0\fswiss Chicago;}{\f2\froman New York;}{\f22\fmodern Courier;}}
{\stylesheet{\f2 \sbasedon222\snext0 Normal;}}\margl1800\margr1800
\pard\plain \f2 Caf\'8e and \'d2quotes\'d3\par
\pard\plain \qc \f22\b Bold Courier\par }";
        let doc = super::rtf::read(rtf).unwrap();
        let texts: Vec<_> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["Café and “quotes”", "Bold Courier"]);
        assert_eq!(
            doc.font_name(doc.main.paras[1].chp_of_char(0).font),
            "Courier"
        );
        assert!(doc.main.paras[1].chp_of_char(0).bold);
        assert_eq!(doc.main.paras[1].pap.align, quill_core::Align::Center);
    }

    #[test]
    fn docx_round_trip() {
        let mut doc = text::from_plain_text("Title\nHello world\nThird");
        let h1 = doc.styles.find("heading 1").unwrap();
        doc.apply_style(StoryId::Main, 0, 0, h1);
        doc.format_chars(
            StoryId::Main,
            Position::new(1, 6),
            Position::new(1, 11),
            |c| c.italic = true,
        );
        doc.format_paras(StoryId::Main, 2, 2, |p| p.align = quill_core::Align::Center);
        doc.summary.title = "Test & check".into();
        let bytes = super::docx::write(&doc, 1_760_000_000);
        let back: Document = super::docx::read(&bytes).unwrap();
        let texts: Vec<_> = back.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["Title", "Hello world", "Third"]);
        assert_eq!(
            back.styles.get(back.main.paras[0].pap.style).name,
            "heading 1"
        );
        assert!(back.main.paras[1].chp_of_char(7).italic);
        assert!(!back.main.paras[1].chp_of_char(0).italic);
        assert_eq!(back.main.paras[2].pap.align, quill_core::Align::Center);
        assert_eq!(back.summary.title, "Test & check");
        assert_eq!(
            back.font_name(back.main.paras[1].chp_of_char(0).font),
            "New York"
        );
    }

    #[test]
    fn docx_keeps_section_and_document_settings() {
        use quill_core::document::FootnotePosition;
        use quill_core::{PageNumberFormat, SectionStart};
        let mut doc = text::from_plain_text("One");
        doc.section.start = SectionStart::OddPage;
        doc.section.columns = 2;
        doc.section.column_spacing = 360;
        doc.section.header_from_top = 1080;
        doc.section.different_first_page = true;
        doc.section.page_number_format = PageNumberFormat::LowerRoman;
        doc.section.restart_page_numbers = true;
        doc.props.margin_left = 2160;
        doc.props.mirror_margins = true;
        doc.props.default_tab = 360;
        doc.props.footnote_position = FootnotePosition::BeneathText;
        doc.props.footnote_restart_each_page = true;
        doc.props.footnote_start = 3;
        doc.format_paras(StoryId::Main, 0, 0, |p| {
            p.borders.top.style = quill_core::props::BorderStyle::Double;
            p.borders.top.space = 80;
            p.shading = 200;
        });
        let back = super::docx::read(&super::docx::write(&doc, 0)).unwrap();
        assert_eq!(back.section, doc.section);
        assert_eq!(back.props, doc.props);
        let p = &back.main.paras[0].pap;
        assert_eq!(p.borders.top.style, quill_core::props::BorderStyle::Double);
        assert_eq!(p.borders.top.space, 80);
        assert_eq!(p.shading, 200);
    }

    #[test]
    fn docx_keeps_headers_footers_and_fields() {
        use quill_core::para::chars;
        let mut doc = text::from_plain_text("Body");
        let mut header = doc.story(StoryId::Header).clone();
        let chp = header.paras[0].mark_chp;
        header.paras[0].insert(0, &format!("Report\tPage {}", chars::FIELD_PAGE), chp);
        doc.set_story(StoryId::Header, header);
        let mut footer = doc.story(StoryId::FirstFooter).clone();
        let chp = footer.paras[0].mark_chp;
        footer.paras[0].insert(
            0,
            &format!("Printed {} at {}", chars::FIELD_DATE, chars::FIELD_TIME),
            chp,
        );
        doc.set_story(StoryId::FirstFooter, footer);
        doc.section.different_first_page = true;
        let bytes = super::docx::write(&doc, 0);
        let back = super::docx::read(&bytes).unwrap();
        assert_eq!(
            back.story(StoryId::Header).paras[0].text,
            format!("Report\tPage {}", chars::FIELD_PAGE)
        );
        assert_eq!(
            back.story(StoryId::FirstFooter).paras[0].text,
            format!("Printed {} at {}", chars::FIELD_DATE, chars::FIELD_TIME)
        );
        assert!(back.story_is_empty(StoryId::Footer));
        assert_eq!(
            back.styles
                .get(back.story(StoryId::Header).paras[0].pap.style)
                .name,
            "header"
        );
        assert_eq!(back.main.paras[0].text, "Body");
    }

    #[test]
    fn docx_reads_other_fields_as_their_results() {
        // A complex field Quill does not know keeps its shown text.
        let xml = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> AUTHOR </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Ada</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:fldSimple w:instr=" PAGE "><w:r><w:t>7</w:t></w:r></w:fldSimple></w:p>"#;
        let doc = super::docx::tests_read_body(xml);
        assert_eq!(
            doc.main.paras[0].text,
            format!("Ada{}", quill_core::para::chars::FIELD_PAGE)
        );
    }

    #[test]
    fn docx_keeps_table_layout() {
        use quill_core::props::BorderStyle;
        use quill_core::table::{row_at, row_props};
        use quill_core::{RowHeight, TABLE_GAP};
        let mut doc = text::from_plain_text("Intro\nEnd");
        doc.insert_table(StoryId::Main, Position::new(1, 0), 2, 3, 2000);
        // Row 1: widen the middle cell, border and shade the first, set a height.
        let r1 = row_at(&doc.main.paras, 1).unwrap();
        let mut p1 = row_props(&doc.main.paras, &r1, 8640);
        p1.cells[1].width = 3000;
        p1.cells[0].borders.top.style = BorderStyle::Single;
        p1.cells[0].shading = 200;
        p1.height = RowHeight::AtLeast(400);
        p1.indent = 360;
        doc.set_row(StoryId::Main, r1.end, p1.clone());
        // Row 2: merge the first two cells.
        let r2 = row_at(&doc.main.paras, r1.end + 1).unwrap();
        let mut p2 = row_props(&doc.main.paras, &r2, 8640);
        p2.indent = 360;
        doc.set_row(StoryId::Main, r2.end, p2.clone());
        let r2 = row_at(&doc.main.paras, r1.end + 1).unwrap();
        doc.merge_cells(StoryId::Main, &r2, &p2, 0, 1);

        let back = super::docx::read(&super::docx::write(&doc, 0)).unwrap();
        let paras = &back.main.paras;
        let b1 = row_at(paras, 1).unwrap();
        let q1 = row_props(paras, &b1, 8640);
        assert_eq!(
            q1.cells.iter().map(|c| c.width).collect::<Vec<_>>(),
            [2000, 3000, 2000]
        );
        assert_eq!(q1.cells[0].borders.top.style, BorderStyle::Single);
        assert_eq!(q1.cells[0].shading, 200);
        assert_eq!(q1.height, RowHeight::AtLeast(400));
        assert_eq!(q1.indent, 360);
        assert_eq!(q1.gap, TABLE_GAP);
        let b2 = row_at(paras, b1.end + 1).unwrap();
        let q2 = row_props(paras, &b2, 8640);
        assert_eq!(
            q2.cells.iter().map(|c| c.width).collect::<Vec<_>>(),
            [4000, 2000]
        );
        assert_eq!(paras.last().unwrap().text, "End");
    }

    /// A small PNG (the header is all the size reading needs).
    fn png() -> Vec<u8> {
        let mut d = b"\x89PNG\r\n\x1a\n".to_vec();
        d.extend_from_slice(&13u32.to_be_bytes());
        d.extend_from_slice(b"IHDR");
        d.extend_from_slice(&40u32.to_be_bytes());
        d.extend_from_slice(&20u32.to_be_bytes());
        d.extend_from_slice(&[8, 2, 0, 0, 0, 1, 2, 3, 4]);
        d.extend_from_slice(b"rest of the file");
        d
    }

    fn doc_with_picture() -> Document {
        let mut doc = text::from_plain_text("Before after");
        let chp = doc.main.paras[0].chp_of_char(0);
        let mut pic = quill_core::Picture::from_bytes(png()).unwrap();
        pic.size = (1200, 300);
        pic.crop.left = 10_000;
        pic.crop.right = 10_000;
        doc.insert_picture(
            quill_core::StoryId::Main,
            quill_core::Position::new(0, 7),
            pic,
            chp,
        );
        doc
    }

    #[test]
    fn pictures_survive_docx_and_rtf() {
        let doc = doc_with_picture();
        let want = doc.picture(1).unwrap().clone();
        let docx = super::docx::read(&super::docx::write(&doc, 0)).unwrap();
        let rtf = super::rtf::read(&super::rtf::write(&doc)).unwrap();
        for back in [docx, rtf] {
            let p = &back.main.paras[0];
            assert_eq!(p.text, "Before \u{FFFC}after");
            let pic = back.picture(p.chp_of_char(7).object).unwrap();
            assert_eq!(pic.data, want.data);
            // RTF keeps the scale in whole percents.
            assert!((pic.size.0 - want.size.0).abs() <= want.size.0 / 100);
            assert!((pic.size.1 - want.size.1).abs() <= want.size.1 / 100);
            assert_eq!(pic.crop, want.crop);
            assert_eq!(p.chp_of_char(10).object, 0);
        }
    }

    #[test]
    fn docx_reuses_one_image_part_for_repeated_placements() {
        let mut doc = doc_with_picture();
        let chp = doc.main.paras[0].mark_chp;
        let second = doc.picture(1).unwrap().scaled_to(600, 150);
        doc.insert_picture(StoryId::Main, Position::new(0, 0), second, chp);
        let bytes = super::docx::write(&doc, 0);
        let archive = super::zip::Archive::open(&bytes).unwrap();
        assert_eq!(
            archive
                .names()
                .filter(|n| n.starts_with("word/media/"))
                .count(),
            1
        );
        let back = super::docx::read(&bytes).unwrap();
        let first = back
            .picture(back.main.paras[0].chp_of_char(0).object)
            .unwrap();
        let second = back
            .picture(back.main.paras[0].chp_of_char(10).object)
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(&first.data, &second.data));
    }

    #[test]
    fn frames_survive_docx_and_rtf() {
        use quill_core::{Frame, FrameH, FrameRel, FrameV};
        let mut doc = text::from_plain_text("O\nnce upon\nPlaced\nEnd");
        let drop = Frame {
            drop_lines: 3,
            from_text: 40,
            ..Default::default()
        };
        let placed = Frame {
            h: FrameH::Right,
            h_rel: FrameRel::Margin,
            v: FrameV::At(1440),
            v_rel: FrameRel::Page,
            width: Some(2880),
            from_text: 180,
            drop_lines: 0,
            in_margin: false,
        };
        doc.main.paras[0].pap.frame = Some(drop);
        doc.main.paras[2].pap.frame = Some(placed);
        let docx = super::docx::read(&super::docx::write(&doc, 0)).unwrap();
        let rtf = super::rtf::read(&super::rtf::write(&doc)).unwrap();
        for back in [docx, rtf] {
            assert_eq!(back.main.paras[0].pap.frame, Some(drop));
            assert_eq!(back.main.paras[1].pap.frame, None);
            assert_eq!(back.main.paras[2].pap.frame, Some(placed));
        }
    }

    #[test]
    fn sections_survive_docx_and_rtf() {
        use quill_core::{PageNumberFormat, SectionStart};
        // Title, a section break, then the body in two columns.
        let mut doc = text::from_plain_text("Title\nBody");
        doc.insert_section_break(Position::new(0, 5));
        doc.format_sections(0, 0, |s| {
            s.columns = 1;
            s.page_number_format = PageNumberFormat::LowerRoman;
        });
        doc.format_sections(2, 2, |s| {
            s.columns = 2;
            s.column_spacing = 360;
            s.start = SectionStart::NoBreak;
            s.restart_page_numbers = true;
            s.line_numbers = quill_core::LineNumbering::Continuous;
            s.line_count_by = 5;
            s.line_from_text = Some(360);
        });
        for back in [
            super::docx::read(&super::docx::write(&doc, 0)).unwrap(),
            super::rtf::read(&super::rtf::write(&doc)).unwrap(),
        ] {
            assert_eq!(back.sections(), doc.sections());
            for p in [0, 2] {
                assert_eq!(back.section_props_at(p), doc.section_props_at(p));
            }
        }
    }

    #[test]
    fn footnotes_survive_docx_and_rtf() {
        use quill_core::document::FootnotePosition;
        use quill_core::notes::note_ids;
        let mut doc = text::from_plain_text("Text with notes\nMore");
        let a = doc.insert_note(Position::new(0, 4), Default::default(), None);
        let b = doc.insert_note(Position::new(1, 4), Default::default(), Some("*".into()));
        let at = doc.note_text_start(a).unwrap();
        doc.insert_text(StoryId::Notes, at, "First note.", Default::default());
        let at = doc.note_text_start(b).unwrap();
        doc.insert_text(StoryId::Notes, at, "Starred.", Default::default());
        let texts = |d: &Document| -> Vec<String> {
            let labels = d.note_labels(None);
            let ranges = d.note_ranges();
            note_ids(&d.main.paras)
                .into_iter()
                .map(|id| {
                    let r = ranges[&id].clone();
                    let body: String = d.notes.paras[r].iter().map(|p| p.text.clone()).collect();
                    format!("{}|{}", labels[&id], body)
                })
                .collect()
        };
        let want = texts(&doc);
        assert_eq!(want.len(), 2);
        for pos in [
            FootnotePosition::BottomOfPage,
            FootnotePosition::EndOfDocument,
        ] {
            doc.props.footnote_position = pos;
            for back in [
                super::docx::read(&super::docx::write(&doc, 0)).unwrap(),
                super::rtf::read(&super::rtf::write(&doc)).unwrap(),
            ] {
                assert_eq!(texts(&back), want);
                assert_eq!(back.props.footnote_position, pos);
                assert_eq!(back.main.paras[0].text, doc.main.paras[0].text);
            }
        }
    }

    #[test]
    fn annotations_survive_docx_and_rtf() {
        use quill_core::notes::{AnnotationInfo, annotation_ids};
        let mut doc = text::from_plain_text("Some text to annotate");
        let info = AnnotationInfo {
            initials: "LK".into(),
            author: "Lucas K".into(),
            date: "2026-10-14".into(),
            ..Default::default()
        };
        let id = doc.insert_annotation(Position::new(0, 4), Default::default(), info.clone());
        let at = doc.annotation_text_start(id).unwrap();
        doc.insert_text(StoryId::Annotations, at, "Check this.", Default::default());
        for (back, dated) in [
            (
                super::docx::read(&super::docx::write(&doc, 0)).unwrap(),
                true,
            ),
            (super::rtf::read(&super::rtf::write(&doc)).unwrap(), false),
        ] {
            let ids = annotation_ids(&back.main.paras);
            assert_eq!(ids.len(), 1);
            assert_eq!(back.annotation_text(ids[0]), "Check this.");
            let got = &back.annotation_info[&ids[0]];
            assert_eq!(
                (got.initials.as_str(), got.author.as_str()),
                ("LK", "Lucas K")
            );
            if dated {
                assert_eq!(got.date, "2026-10-14");
            }
            assert_eq!(
                back.main.paras[0]
                    .text
                    .replace(quill_core::para::chars::ANNOTATION, ""),
                "Some text to annotate"
            );
        }
    }
}
