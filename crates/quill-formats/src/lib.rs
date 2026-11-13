//! Reading and writing Quill documents.

mod character_styles;
mod comments;
pub mod docx;
mod export;
pub mod html;
mod html_entities;
mod html_read;
pub mod images;
pub mod markdown;
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

mod atomic;
#[allow(unused_imports)]
use atomic::*;
pub use atomic::{copy_atomic, write_atomic};

use quill_core::Document;
use quill_core::limits::Size;
use quill_core::para::Paragraph;
use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_NUMBER: AtomicU64 = AtomicU64::new(0);

/// Counts allocations while a format reader builds a document. The first
/// paragraph that crosses a limit is retained so the caller can report the
/// ordinary document-size error, but no later paragraph is built.
#[derive(Default)]
struct ReadBudget {
    size: Size,
    pictures: HashSet<String>,
    full: bool,
}

impl ReadBudget {
    fn add_paragraph(&mut self, paragraph: &Paragraph) -> bool {
        if self.full {
            return false;
        }
        self.size.paragraphs = self.size.paragraphs.saturating_add(1);
        self.size.characters = self
            .size
            .characters
            .saturating_add(paragraph.text.chars().count());
        self.size.runs = self.size.runs.saturating_add(paragraph.runs.len());
        self.full = self.size.too_large().is_some();
        true
    }

    /// Notes that a paragraph list now holds `n` paragraphs. For readers that
    /// build paragraphs in bulk (repeated table cells), where charging each
    /// one is not practical. Returns false once a limit is crossed.
    fn note_paragraph_count(&mut self, n: usize) -> bool {
        if n > self.size.paragraphs {
            self.size.paragraphs = n;
            self.full = self.size.too_large().is_some();
        }
        !self.full
    }

    fn add_picture(&mut self, key: &str) -> bool {
        if self.pictures.contains(key) {
            return true;
        }
        if self.full {
            return false;
        }
        self.pictures.insert(key.to_string());
        self.size.pictures = self.pictures.len();
        self.full = self.size.too_large().is_some();
        true
    }

    fn full(&self) -> bool {
        self.full
    }

    fn too_large(&self) -> Option<String> {
        self.size.too_large()
    }
}

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
    /// OpenDocument Text.
    Odt,
    /// A self-contained HTML document.
    Html,
    /// Markdown: headings, emphasis, lists, quotes, code, links, tables and
    /// pictures.
    Markdown,
    /// Plain text.
    Text,
}

impl Format {
    pub const ALL: [Format; 7] = [
        Format::Quill,
        Format::Docx,
        Format::Rtf,
        Format::Odt,
        Format::Html,
        Format::Markdown,
        Format::Text,
    ];

    /// The name the Save As dialog shows.
    pub fn name(self) -> &'static str {
        match self {
            Format::Quill => "Quill Document",
            Format::Docx => "Word Document",
            Format::Rtf => "Interchange Format (RTF)",
            Format::Odt => "OpenDocument Text",
            Format::Html => "HTML Document",
            Format::Markdown => "Markdown",
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
            Format::Markdown => &["md", "markdown"],
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
    "qwp", "docx", "docm", "dotx", "dotm", "rtf", "odt", "ott", "odm", "fodt", "html", "htm", "md",
    "markdown", "txt", "text",
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
    load_reported_trusting(path, interpret_rtf, false)
}

/// `load_reported`, with `trust_outside_pictures` true to also load the
/// pictures a Markdown file names outside its own folder. The report of a
/// normal load lists those (`outside_pictures`) so the person can be asked.
pub fn load_reported_trusting(
    path: &Path,
    interpret_rtf: bool,
    trust_outside_pictures: bool,
) -> Result<(Document, report::ImportReport), String> {
    // Whatever the file says, its measurements end up within Word's range,
    // and it's no bigger than Quill can save and open again.
    let (mut doc, report) = read_any(path, interpret_rtf, trust_outside_pictures)?;
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
    if utf16(bytes).is_some() || unmarked_utf16(bytes).is_some() {
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

/// Text in UTF-16 without a byte-order mark, as some Windows programs
/// write it: told by the zero byte beside nearly every letter of English
/// and other Latin-script text.
fn unmarked_utf16(bytes: &[u8]) -> Option<String> {
    let head = &bytes[..bytes.len().min(8192) & !1];
    if head.len() < 4 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let pairs = head.len() / 2;
    let zero_at = |k: usize| head.iter().skip(k).step_by(2).filter(|&&b| b == 0).count();
    let (even, odd) = (zero_at(0), zero_at(1));
    let big = if odd * 10 >= pairs * 7 && even * 10 <= pairs {
        false
    } else if even * 10 >= pairs * 7 && odd * 10 <= pairs {
        true
    } else {
        return None;
    };
    let units: Vec<u16> = bytes
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
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return false;
    }
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

/// A file's text: UTF-16 or UTF-8 as it says, otherwise Windows' Western
/// encoding.
fn decode_text(bytes: Vec<u8>) -> String {
    // UTF-16 first: its zero bytes can pass for UTF-8.
    let text = match utf16(&bytes).or_else(|| unmarked_utf16(&bytes)) {
        Some(text) => Ok(text),
        None => String::from_utf8(bytes),
    };
    match text {
        Ok(s) => s,
        // Otherwise Windows' Western encoding (Windows-1252), which is
        // Latin-1 with curly quotes, dashes and the euro sign in 0x80-0x9F.
        Err(e) => e
            .into_bytes()
            .iter()
            .map(|&b| match b {
                0x80..=0x9F => crate::rtf::read::CP1252[(b - 0x80) as usize],
                b => b as char,
            })
            .collect(),
    }
}

/// A file read as plain text, checked for size before any paragraphs are
/// made, since a huge file of short lines would otherwise fill memory.
fn plain_text(path: &Path, bytes: Vec<u8>) -> Result<Document, String> {
    let text = decode_text(bytes);
    too_large(path, quill_core::limits::Size::of_text(&text))?;
    Ok(quill_core::text::from_plain_text(&text))
}

fn read_any(
    path: &Path,
    interpret_rtf: bool,
    trust_outside_pictures: bool,
) -> Result<(Document, report::ImportReport), String> {
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
    if Format::from_path(path) == Some(Format::Markdown) && looks_like_text(&bytes) {
        let text = decode_text(bytes);
        too_large(path, quill_core::limits::Size::of_text(&text))?;
        let (doc, mut report) =
            markdown::read_trusting(&text, path.parent(), trust_outside_pictures);
        report.no_page = true;
        return Ok((doc, report));
    }
    if odt_read::is_flat(&bytes) {
        return odt_read::read_flat_reported(&bytes).map_err(|e| document_error(path, "ODT", e));
    }
    if html_read::is_html(&bytes) || Format::from_path(path) == Some(Format::Html) {
        return html_read::read_file(&bytes)
            .map(|(doc, mut report)| {
                report.no_page = true;
                (doc, report)
            })
            .map_err(|e| document_error(path, "HTML", e));
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
    plain_text(path, bytes).map(|doc| {
        let (doc, mut report) = plain(doc);
        report.no_page = true;
        (doc, report)
    })
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
/// Read a whole file of at most `limit` bytes. Anything but a regular file
/// (a pipe, a device, a directory) is refused before it is opened, so
/// picking one cannot block or fill memory.
pub fn read_file_with_limit(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    // Opening a named pipe can block before any byte limit is applied.
    // Reject devices, pipes and directories before opening, then check the
    // opened handle too in case the path changed.
    let metadata = std::fs::metadata(path).map_err(|e| file_error("inspect", path, e))?;
    if !metadata.is_file() {
        return Err(format!("Not a regular file: {}", path.display()));
    }
    let file = std::fs::File::open(path)
        .map_err(|e| format!("Could not open \u{201c}{}\u{201d}: {e}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|e| file_error("inspect", path, e))?;
    if !metadata.is_file() {
        return Err(format!("Not a regular file: {}", path.display()));
    }
    let len = metadata.len();
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
    save_reading_back(doc, path, format, false).map(drop)
}

/// Save as .qwp even though the file does not read back exactly as the
/// document is (see `native_check`). The file must still open. Returns
/// the document as it reads back, which is what the file now holds.
pub fn save_anyway(doc: &Document, path: &Path) -> Result<Document, String> {
    save_reading_back(doc, path, Format::Quill, true)
        .map(|back| back.expect("a .qwp save reads back"))
}

/// `save`, and for .qwp the document as it reads back. With `accept_loss`
/// a .qwp is saved even when that differs from `doc`.
fn save_reading_back(
    doc: &Document,
    path: &Path,
    format: Format,
    accept_loss: bool,
) -> Result<Option<Document>, String> {
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
    let doc = if matches!(
        format,
        Format::Odt | Format::Html | Format::Markdown | Format::Text
    ) && doc.has_changes()
    {
        accepted = {
            let mut d = doc.compact_for_save();
            d.resolve_all_changes(true);
            d
        };
        &accepted
    } else {
        doc
    };
    // Markdown keeps its pictures as files in a folder beside it.
    let mut pictures: Vec<markdown::PictureFile> = Vec::new();
    let bytes = match format {
        Format::Quill => native::write(doc, now),
        Format::Docx => docx::write(doc, now),
        Format::Rtf => rtf::write(doc),
        Format::Odt => odt::write(doc, now),
        Format::Html => html::write(doc),
        Format::Markdown => {
            let (text, files) = markdown::write_with_pictures(doc, &markdown_pictures_folder(path));
            pictures = files;
            text.into_bytes()
        }
        Format::Text => {
            let eol = if cfg!(windows) { "\r\n" } else { "\n" };
            quill_core::text::to_plain_text(doc, eol).into_bytes()
        }
    };
    // Read the result back before it replaces anything, so a save that
    // couldn't be opened again never reaches the disk.
    let mut read_back = None;
    let back = match format {
        // Quill's own format keeps everything, so what comes back must be
        // what was saved. A difference means a bug in the writer or the
        // reader, and the file on disk is better left as it was, unless
        // the person chose to save anyway.
        Format::Quill => native::read(&bytes).and_then(|back| {
            match native_loss(doc, &back) {
                Some(loss) if !accept_loss => return Err(loss.to_string()),
                _ => {}
            }
            read_back = Some(back);
            Ok(())
        }),
        Format::Docx => docx::read(&bytes).map(drop),
        Format::Rtf => rtf::read(&bytes).map(drop),
        Format::Odt => odt_read::read(&bytes).map(drop),
        Format::Html => html_read::read_file(&bytes).map(drop),
        // Markdown is read back from a file, so the result must still be
        // within what Quill opens.
        Format::Markdown => match markdown::read(&String::from_utf8_lossy(&bytes), None)
            .0
            .size()
            .too_large()
        {
            Some(why) => Err(format!("the result is too large: {why}")),
            None => Ok(()),
        },
        Format::Text => Ok(()),
    };
    if let Err(e) = back {
        return Err(format!(
            "Quill didn't save \u{201c}{file}\u{201d}, because it couldn't read the result back \
             correctly ({e}). The file on disk is unchanged.{}",
            if format == Format::Quill {
                " Saving a copy in another format may work."
            } else {
                " Saving in Quill's own format may work."
            }
        ));
    }
    if !pictures.is_empty() {
        // The pictures go in first, so the Markdown never names a file that
        // is not there.
        let folder = path.with_file_name(markdown_pictures_folder(path));
        std::fs::create_dir_all(&folder).map_err(|e| file_error("create", &folder, e))?;
        if !std::fs::symlink_metadata(&folder)
            .map_err(|e| file_error("inspect", &folder, e))?
            .file_type()
            .is_dir()
        {
            return Err(format!(
                "Picture folder is not a regular directory: {}",
                folder.display()
            ));
        }
        for picture in &pictures {
            write_picture_file(&folder.join(&picture.name), &picture.bytes)?;
        }
    }
    write_atomic(path, &bytes)?;
    Ok(read_back)
}

/// Where a .qwp read back differs from the document that was saved.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeLoss {
    /// The part of the document, such as "text" or "styles".
    pub part: &'static str,
    /// In the main text, the first paragraph that differs, so the app can
    /// show it.
    pub para: Option<usize>,
    /// Which paragraph and what changed in it, for the message.
    pub detail: String,
}

impl std::fmt::Display for NativeLoss {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "the {} came back different", self.part)?;
        if !self.detail.is_empty() {
            write!(f, " {}", self.detail)?;
        }
        Ok(())
    }
}

/// Save `doc` as .qwp in memory and read it back, and say where the result
/// differs, if it does, with the document as it reads back. The app asks
/// this after a refused save so it can select the paragraph at fault and
/// offer to save anyway.
pub fn native_check(doc: &Document) -> Option<(NativeLoss, Document)> {
    let back = native::read(&native::write(doc, 0)).ok()?;
    native_loss(doc, &back).map(|loss| (loss, back))
}

/// Where a .qwp read back as `back` does not keep `doc`, or None if it kept
/// every part. The save time and other details the writer fills in are
/// left out.
fn native_loss(doc: &Document, back: &Document) -> Option<NativeLoss> {
    let whole = |part| NativeLoss {
        part,
        para: None,
        detail: String::new(),
    };
    if let Some((para, detail)) = story_loss(&doc.main.paras, &back.main.paras) {
        return Some(NativeLoss {
            part: "text",
            para: Some(para),
            detail,
        });
    }
    if let Some((_, detail)) = story_loss(&doc.notes.paras, &back.notes.paras) {
        return Some(NativeLoss {
            detail,
            ..whole("footnotes")
        });
    }
    if doc.headers.len() != back.headers.len() {
        return Some(whole("headers and footers"));
    }
    for (a, b) in doc.headers.iter().zip(&back.headers) {
        if let Some((_, detail)) = story_loss(&a.paras, &b.paras) {
            return Some(NativeLoss {
                detail,
                ..whole("headers and footers")
            });
        }
    }
    if let Some((_, detail)) = story_loss(&doc.annotations.paras, &back.annotations.paras) {
        return Some(NativeLoss {
            detail,
            ..whole("comments")
        });
    }
    [
        ("styles", back.styles == doc.styles),
        ("document settings", back.props == doc.props),
        ("section settings", back.section == doc.section),
        ("pictures", back.objects == doc.objects),
        ("fonts", back.fonts == doc.fonts),
    ]
    .into_iter()
    .find(|(_, same)| !same)
    .map(|(part, _)| whole(part))
}

/// The first paragraph of `a` that `b` does not match, with a description
/// of it and of what changed, or None if they match.
fn story_loss(a: &[quill_core::Paragraph], b: &[quill_core::Paragraph]) -> Option<(usize, String)> {
    let i = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i))?;
    let what = match (a.get(i), b.get(i)) {
        (Some(x), Some(y)) if x.text != y.text => "its text changed",
        (Some(x), Some(y)) if x.runs != y.runs || x.mark_chp != y.mark_chp => {
            "its character formatting changed"
        }
        (Some(x), Some(y)) if x.pap != y.pap || x.mark != y.mark => {
            "its paragraph formatting changed"
        }
        (Some(x), Some(y)) if x.row != y.row => "its table row settings changed",
        (Some(_), Some(_)) => "its section settings changed",
        (Some(_), None) => "it was missing",
        (None, _) => "an extra paragraph came after it",
    };
    let shown: String = a
        .get(i)
        .or(a.last())
        .map(|p| p.text.as_str())
        .unwrap_or("")
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let shown = shown.trim();
    let quote = if shown.is_empty() {
        "an empty paragraph".to_string()
    } else if shown.chars().count() > 40 {
        let cut: String = shown.chars().take(40).collect();
        format!("\u{201c}{}\u{2026}\u{201d}", cut.trim_end())
    } else {
        format!("\u{201c}{shown}\u{201d}")
    };
    let at = i.min(a.len().saturating_sub(1));
    Some((at, format!("at paragraph {} ({quote}): {what}", at + 1)))
}

// Assets are immutable once published: even an unlikely checksum collision
// or a user-owned file must not overwrite pictures used by an older save.
fn write_picture_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata =
                std::fs::symlink_metadata(path).map_err(|e| file_error("inspect", path, e))?;
            if metadata.file_type().is_file() && metadata.len() == bytes.len() as u64 {
                let mut existing = Vec::new();
                std::fs::File::open(path)
                    .and_then(|file| file.take(bytes.len() as u64 + 1).read_to_end(&mut existing))
                    .map_err(|e| file_error("read", path, e))?;
                if existing == bytes {
                    return Ok(());
                }
            }
            return Err(format!(
                "Quill did not replace the existing picture file: {}",
                path.display()
            ));
        }
        Err(e) => return Err(file_error("create", path, e)),
    };
    let mut cleanup = TempCleanup {
        path: Some(path.to_path_buf()),
        file: Some(file),
    };
    let file = cleanup.file.as_mut().unwrap();
    file.write_all(bytes)
        .map_err(|e| file_error("write", path, e))?;
    file.sync_all().map_err(|e| file_error("sync", path, e))?;
    drop(cleanup.file.take());
    cleanup.path = None;
    sync_parent(path)
}

/// The folder a Markdown file's pictures are saved in, beside it: its name
/// with "_images" added, and anything but letters, digits, "-" and "_" in
/// the name made "_" so the file's links need no escaping.
pub fn markdown_pictures_folder(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim_matches('_');
    if safe.is_empty() {
        "images".into()
    } else {
        format!("{safe}_images")
    }
}

/// Save Markdown text exactly as it is: for a document being edited as
/// Markdown source, where the text is the file.
pub fn save_markdown_text(text: &str, path: &Path) -> Result<(), String> {
    let file = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    if let Some(why) = quill_core::limits::Size::of_text(text).too_large() {
        return Err(format!("Quill didn't save \u{201c}{file}\u{201d}: {why}."));
    }
    write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests;
