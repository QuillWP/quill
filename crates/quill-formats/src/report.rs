//! What a reader could not keep exactly, worded for the warning Quill shows
//! after opening a file. Each note is one line of that warning.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    notes: Vec<&'static str>,
    /// The file's format has no page size or margins (HTML, Markdown, plain
    /// text), so the ones it came with are only Quill's built-in defaults.
    pub no_page: bool,
}

impl ImportReport {
    /// Note `what`, once however often it happens.
    pub fn note(&mut self, what: &'static str) {
        if !self.notes.contains(&what) {
            self.notes.push(what);
        }
    }

    /// Take back a note that turned out not to apply.
    pub fn forget(&mut self, what: &'static str) {
        self.notes.retain(|n| *n != what);
    }

    pub fn notes(&self) -> &[&'static str] {
        &self.notes
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }
}

pub const TRACKED_CHANGES: &str = "Tracked changes were accepted; their history was not kept.";
pub const FORMAT_CHANGES: &str = "Tracked changes to paragraph, table and section formatting were \
     accepted; Quill keeps tracked insertions, deletions and character formatting.";
pub const CHARACTER_STYLES: &str =
    "Some character styles could not be kept as editable named styles.";
pub const SHADING_COLORS: &str = "Some shading colors became gray patterns.";
pub const NUMBERING: &str =
    "Automatic numbering and bullets were kept as typed text and will not renumber.";
pub const TEXT_BOXES: &str =
    "Text boxes became frames, or where that was not possible their text follows its paragraph.";
pub const TEXT_BOXES_LOST: &str = "Text boxes were left out, along with their text.";
pub const DRAWINGS: &str = "Drawings and shapes were left out.";
pub const PICTURES: &str =
    "Some pictures are in a format Quill cannot show (such as EMF, WMF or SVG) and were left out.";
pub const OBJECTS: &str = "Embedded objects such as charts and spreadsheets were left out, or kept only as their picture.";
pub const EQUATIONS: &str = "Equations were kept as a line of text, such as (a+b)/2, and can no longer be edited as equations.";
pub const ANCHORED: &str =
    "Floating pictures became frames, which may not sit exactly where they did.";
pub const NESTED_TABLES: &str = "Tables nested more than five deep, or inside a paragraph, were flattened into the cell they are in.";
pub const WIDE_ROWS: &str =
    "Table rows with more than 255 cells had the extra cells' text put in the last cell.";
pub const FIELDS: &str =
    "Fields other than page number, date and time were kept as their current text.";
pub const HYPERLINKS: &str = "Links to places within the document were kept as plain text.";
pub const BOOKMARKS: &str = "Bookmarks were left out.";
pub const INDEX_ENTRIES: &str = "Index and table of contents entries were left out.";
pub const GENERATED_INDEXES: &str =
    "Tables of contents and indexes were kept as text and will not update.";
pub const CONTROLS: &str = "Content controls and form fields were kept as plain text.";
pub const PAGE_STYLES: &str =
    "Parts of the document use other page sizes or margins; one set is used throughout.";
pub const ENDNOTES: &str = "Endnotes were kept as footnotes.";
pub const RUBY: &str =
    "Ruby text (small reading guides above characters) was put in brackets after its characters.";
pub const ASIDE_NOTES: &str =
    "Footnotes in headers, footers or other notes were kept as ordinary text.";
pub const ASIDE_COMMENTS: &str = "Comments in headers, footers or other comments were left out.";
pub const NOTE_COMMENTS: &str =
    "Comments in footnotes were moved to the footnote's mark in the text.";
pub const EMBEDDED_DOCUMENTS: &str = "Embedded documents were left out.";
pub const MACROS: &str = "Macros were left out.";
pub const LINKED_SECTIONS: &str = "Sections linked to other files (as in a master document) show the copy saved in this file; the linked files were not opened.";
pub const STYLE_SHEETS: &str =
    "Style sheets the page links to were not read, so some of its formatting may be missing.";
pub const LINKED_PICTURES: &str =
    "Pictures the page links to were left out; Quill never fetches them.";
pub const MARKDOWN_LINKED_PICTURES: &str =
    "Pictures the Markdown links to on the web were kept as links; Quill never fetches them.";
pub const MARKDOWN_HTML: &str = "HTML written in the Markdown was kept as text, apart from underline, superscript, subscript and line breaks.";
pub const MARKDOWN_NOTES: &str = "Footnotes written as [^1] were kept as ordinary text.";
pub const MARKDOWN_PICTURES: &str =
    "Pictures the file names were left out where Quill could not find them beside the file.";
pub const ENCODING: &str =
    "The page uses a character set Quill does not read, so some characters may be wrong.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_note_is_kept_once_in_order() {
        let mut r = ImportReport::default();
        assert!(r.is_empty());
        r.note(FIELDS);
        r.note(DRAWINGS);
        r.note(FIELDS);
        assert_eq!(r.notes(), [FIELDS, DRAWINGS]);
    }
}
