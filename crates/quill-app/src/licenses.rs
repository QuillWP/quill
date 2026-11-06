//! Quill's licenses, as one text for the Licenses window: its own, then the
//! notices for the data and the libraries it carries. Everything is read from
//! the files that travel with the program (in a macOS bundle's Resources, or
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
    for (title, name) in [
        ("Quill: the MIT license", "LICENSE-MIT"),
        ("Quill: the Apache License 2.0", "LICENSE-APACHE"),
        (
            "Thesaurus data: WordNet, Princeton University",
            "data/licenses/WordNet_license.txt",
        ),
        (
            "Hyphenation patterns",
            "data/licenses/README_hyph_en_US.txt",
        ),
    ] {
        if let Some(body) = read(name) {
            heading(&mut out, title);
            out.push_str(body.trim());
            out.push('\n');
        }
    }
    if let Some(body) = read("THIRD-PARTY-NOTICES.txt") {
        heading(&mut out, "Libraries Quill is built from");
        // The notices file ends with the data licenses, shown above.
        let body = body
            .split_once("\nThesaurus data (WordNet, Princeton University)\n")
            .map_or(body.as_str(), |(libraries, _)| libraries);
        out.push_str(body.trim());
        out.push('\n');
    }
    if out.lines().count() < 12 {
        out.push_str(
            "\nThe license files that come with Quill (LICENSE-MIT, LICENSE-APACHE and \
THIRD-PARTY-NOTICES.txt) were not found next to the program.\n",
        );
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
    fn the_text_holds_quills_licenses_then_the_libraries() {
        let t = text();
        // Run from the source tree, the files are all there.
        assert!(t.starts_with("Quill\n===="));
        assert!(t.contains("MIT License"));
        assert!(t.contains("Apache License"));
        assert!(t.contains("WordNet 2.1 Copyright 2005 by Princeton University"));
        assert!(t.contains("Libraries Quill is built from"));
        assert!(!t.contains("were not found"));
        // The data notices are not listed twice.
        assert_eq!(t.matches("WordNet 2.1 Copyright 2005").count(), 1);
        // Each heading is underlined to its own width.
        assert!(t.contains("Quill: the MIT license\n======================\n"));
    }
}
