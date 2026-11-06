//! Quill's licenses, as one text for the Licenses window: its own two, and a
//! pointer to the notices for the data and libraries in this build, which
//! are long enough to be a file of their own. Everything is read from the
//! files that travel with the program (in a macOS bundle's Resources, or
//! beside the program, or in the source tree when run from a build), so it
//! works offline.

use std::path::PathBuf;

/// The one-sentence summary the About box shows above its link.
pub const SUMMARY: &str = "Quill is available under either the MIT license or the Apache License 2.0. \
Certain parts of Quill use code and data licensed under other terms, including WordNet \
(Princeton University) and many open-source libraries.";

/// What the About box's link says.
#[cfg(target_os = "macos")]
pub const LINK_TEXT: &str = "For a full review of licenses, click here.";

/// The line in the Licenses window that leads to the notices file.
pub const NOTICES_LINK: &str = "Licenses for the libraries and data in this copy of Quill";

/// The notices file, listing what this platform's build is made from.
pub fn notices_path() -> Option<PathBuf> {
    dirs()
        .into_iter()
        .map(|d| d.join("THIRD-PARTY-NOTICES.txt"))
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
}

/// Where a license file may be: beside the program, in a macOS bundle's
/// Resources, or (when run from a build) in the source tree.
fn dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        out.push(dir.to_path_buf());
        out.push(dir.join("../Resources"));
        out.push(dir.join("../.."));
        out.push(dir.join("../../.."));
    }
    out.push(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")));
    out
}

/// A license file's text, searched for in `dirs`, under `name`.
fn read(name: &str) -> Option<String> {
    dirs()
        .into_iter()
        .find_map(|d| std::fs::read_to_string(d.join(name)).ok())
}

/// A heading above one license, underlined.
fn heading(out: &mut String, title: &str) {
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(title);
    out.push('\n');
    out.push_str(&"=".repeat(title.chars().count()));
    out.push_str("\n\n");
}

/// The text of the Licenses window.
pub fn text() -> String {
    let mut out = String::new();
    heading(&mut out, "Quill");
    out.push_str(SUMMARY);
    out.push('\n');
    if let Some(path) = notices_path() {
        out.push('\n');
        out.push_str(NOTICES_LINK);
        // The Mac's window makes the line a link. Elsewhere the text opens
        // in a plain editor, so it gives the place.
        #[cfg(not(target_os = "macos"))]
        out.push_str(&format!(": {}", path.display()));
        #[cfg(target_os = "macos")]
        let _ = path;
        out.push('\n');
    }
    for (title, name) in [
        ("Quill: the MIT license", "LICENSE-MIT"),
        ("Quill: the Apache License 2.0", "LICENSE-APACHE"),
    ] {
        if let Some(body) = read(name) {
            heading(&mut out, title);
            out.push_str(body.trim());
            out.push('\n');
        }
    }
    if out.lines().count() < 12 {
        out.push_str(
            "\nThe license files that come with Quill (LICENSE-MIT, LICENSE-APACHE and \
THIRD-PARTY-NOTICES.txt) were not found next to the program.\n",
        );
    }
    out
}

/// Where each web address in `text` starts and ends.
#[cfg(target_os = "macos")]
pub fn web_addresses(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find("http") {
        let at = from + found;
        let word = &text[at..];
        let len = word
            .find(|c: char| c.is_whitespace() || "<>()\"".contains(c))
            .unwrap_or(word.len());
        let address = word[..len].trim_end_matches(['.', ',', ';']);
        if address.starts_with("https://") || address.starts_with("http://") {
            out.push((at, at + address.len()));
        }
        from = at + len.max(4);
    }
    out
}

/// A stand-in for systems without a Licenses window yet: write the text to a
/// file and open it in the program the system uses for text.
#[cfg(not(target_os = "macos"))]
pub fn open_text_file() -> bool {
    let path = std::env::temp_dir().join("Quill Licenses.txt");
    if std::fs::write(&path, text()).is_err() {
        return false;
    }
    #[cfg(target_os = "windows")]
    let opener = std::process::Command::new("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(&path)
        .spawn();
    #[cfg(not(target_os = "windows"))]
    let opener = std::process::Command::new("xdg-open").arg(&path).spawn();
    opener.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn web_addresses_stop_at_their_punctuation() {
        let text = "see https://a.org/x. And (http://b.org) not httpd";
        let found: Vec<&str> = web_addresses(text)
            .into_iter()
            .map(|(a, b)| &text[a..b])
            .collect();
        assert_eq!(found, ["https://a.org/x", "http://b.org"]);
    }

    #[test]
    fn the_text_holds_quills_licenses_and_points_to_the_rest() {
        let t = text();
        // Run from the source tree, the files are all there.
        assert!(t.starts_with("Quill\n===="));
        assert!(t.contains("MIT License"));
        assert!(t.contains("Apache License"));
        assert!(!t.contains("were not found"));
        // The rest is in the notices file, which the text leads to.
        assert!(t.contains(NOTICES_LINK));
        assert!(!t.contains("WordNet 2.1 Copyright 2005"));
        let notices = std::fs::read_to_string(notices_path().unwrap()).unwrap();
        assert!(notices.contains("WordNet 2.1 Copyright 2005"));
        // Each heading is underlined to its own width.
        assert!(t.contains("Quill: the MIT license\n======================\n"));
    }
}
