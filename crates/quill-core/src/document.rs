//! The document and every edit that changes it.
//!
//! All changes go through `splice`, which replaces a range of paragraphs in
//! a story. That one primitive gives undo, redo and the change log the
//! layout cache uses, so no edit can forget either.

use crate::para::{Mark, Paragraph, chars};
use crate::picture::{Object, Picture};
use crate::props::{CharProps, FontId, ParaProps, StyleId, Twips};
use crate::story::{Position, Selection, Story};
use crate::styles::StyleSheet;

/// Which story an edit applies to: the main text, one of the headers and
/// footers, or the footnotes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StoryId {
    #[default]
    Main,
    Header,
    Footer,
    FirstHeader,
    FirstFooter,
    EvenHeader,
    EvenFooter,
    /// Every footnote's text, in the order of the references.
    Notes,
    /// Every annotation's text, in the order of the marks.
    Annotations,
}

impl StoryId {
    /// The headers and footers, in the order `Document::headers` keeps them.
    pub const HEADERS: [StoryId; 6] = [
        StoryId::Header,
        StoryId::Footer,
        StoryId::FirstHeader,
        StoryId::FirstFooter,
        StoryId::EvenHeader,
        StoryId::EvenFooter,
    ];

    pub fn is_footer(self) -> bool {
        matches!(
            self,
            StoryId::Footer | StoryId::FirstFooter | StoryId::EvenFooter
        )
    }

    /// The name the original gives the pane, as in "Untitled1:Header".
    pub fn name(self) -> &'static str {
        match self {
            StoryId::Main => "",
            StoryId::Header => "Header",
            StoryId::Footer => "Footer",
            StoryId::FirstHeader => "First Header",
            StoryId::FirstFooter => "First Footer",
            StoryId::EvenHeader => "Even Header",
            StoryId::EvenFooter => "Even Footer",
            StoryId::Notes => "Footnotes",
            StoryId::Annotations => "Annotations",
        }
    }

    /// The place of a header or footer in `HEADERS`, None for the other
    /// stories.
    pub fn header_index_checked(self) -> Option<usize> {
        StoryId::HEADERS.iter().position(|&s| s == self)
    }

    pub fn header_index(self) -> usize {
        StoryId::HEADERS
            .iter()
            .position(|&s| s == self)
            .unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FootnotePosition {
    #[default]
    BottomOfPage,
    BeneathText,
    EndOfSection,
    EndOfDocument,
}

/// Where a section starts (Format > Section > Start).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SectionStart {
    #[default]
    NoBreak,
    NewColumn,
    NewPage,
    EvenPage,
    OddPage,
}

/// Section > Line Numbers: when the count starts again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineNumbering {
    #[default]
    Off,
    ByPage,
    BySection,
    Continuous,
}

/// How page numbers are written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PageNumberFormat {
    #[default]
    Arabic,
    UpperRoman,
    LowerRoman,
    UpperLetter,
    LowerLetter,
}

impl PageNumberFormat {
    /// Page number `n` written in this format.
    pub fn text(self, n: u32) -> String {
        use PageNumberFormat as F;
        let letters = |upper: bool| {
            // A..Z, then AA..ZZ, as the original counts.
            let n = n.max(1) - 1;
            let c = (b'A' + (n % 26) as u8) as char;
            let s: String = std::iter::repeat_n(c, (n / 26 + 1) as usize).collect();
            if upper { s } else { s.to_lowercase() }
        };
        let roman = |upper: bool| {
            let mut n = n.max(1);
            let mut s = String::new();
            for (v, r) in [
                (1000, "M"),
                (900, "CM"),
                (500, "D"),
                (400, "CD"),
                (100, "C"),
                (90, "XC"),
                (50, "L"),
                (40, "XL"),
                (10, "X"),
                (9, "IX"),
                (5, "V"),
                (4, "IV"),
                (1, "I"),
            ] {
                while n >= v {
                    s.push_str(r);
                    n -= v;
                }
            }
            if upper { s } else { s.to_lowercase() }
        };
        match self {
            F::Arabic => n.to_string(),
            F::UpperRoman => roman(true),
            F::LowerRoman => roman(false),
            F::UpperLetter => letters(true),
            F::LowerLetter => letters(false),
        }
    }
}

/// Format > Section settings. Each section break carries the settings of
/// the section it ends; the last section's are `Document::section`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SectionProps {
    pub start: SectionStart,
    pub columns: u16,
    pub column_spacing: Twips,
    /// Header distance from the top of the page, footer from the bottom.
    pub header_from_top: Twips,
    pub footer_from_bottom: Twips,
    pub different_first_page: bool,
    pub page_number_format: PageNumberFormat,
    pub restart_page_numbers: bool,
    /// Page numbers printed in the margin without a header or footer.
    pub margin_page_numbers: bool,
    pub page_number_from_top: Twips,
    pub page_number_from_right: Twips,
    pub include_endnotes: bool,
    /// Line numbers in the left margin, every `line_count_by` lines,
    /// `line_from_text` from the text (None for the original's automatic 0.25 in).
    pub line_numbers: LineNumbering,
    pub line_count_by: u16,
    pub line_from_text: Option<Twips>,
    /// This section's own headers and footers, in `StoryId::HEADERS`
    /// order, or None for Same As Previous: the nearest section before
    /// with its own. The first section's are `Document::headers`, so this
    /// is None there.
    pub headers: Option<std::sync::Arc<Vec<Story>>>,
}

impl Default for SectionProps {
    /// The original's defaults, as its Section dialog shows them.
    fn default() -> Self {
        SectionProps {
            start: SectionStart::NoBreak,
            columns: 1,
            column_spacing: 720,
            header_from_top: 720,
            footer_from_bottom: 720,
            different_first_page: false,
            page_number_format: PageNumberFormat::Arabic,
            restart_page_numbers: false,
            margin_page_numbers: false,
            page_number_from_top: 720,
            page_number_from_right: 720,
            include_endnotes: false,
            line_numbers: LineNumbering::Off,
            line_count_by: 1,
            line_from_text: None,
            headers: None,
        }
    }
}

/// Format > Document settings, plus the page size from Page Setup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentProps {
    pub page_width: Twips,
    pub page_height: Twips,
    pub margin_left: Twips,
    pub margin_right: Twips,
    pub margin_top: Twips,
    pub margin_bottom: Twips,
    /// "Exactly" instead of "At Least" for the top and bottom margins.
    pub top_exact: bool,
    pub bottom_exact: bool,
    pub gutter: Twips,
    pub mirror_margins: bool,
    pub default_tab: Twips,
    pub widow_control: bool,
    pub print_hidden: bool,
    pub even_odd_headers: bool,
    pub footnote_position: FootnotePosition,
    pub footnote_restart_each_page: bool,
    pub footnote_start: u16,
    /// Record insertions, deletions and format changes as tracked changes
    /// (Tools > Track Changes).
    pub track_changes: bool,
    /// The table of contents at the start is live: Quill keeps its entries
    /// and page numbers up to date, made with these settings. None once it
    /// is made editable, or when there is none.
    pub live_toc: Option<crate::contents::TocOptions>,
}

impl Default for DocumentProps {
    /// The original's defaults: US Letter, 1.25 in left and right margins, 1 in
    /// top and bottom, tabs every half inch.
    fn default() -> Self {
        DocumentProps {
            page_width: 12240,
            page_height: 15840,
            margin_left: 1800,
            margin_right: 1800,
            margin_top: 1440,
            margin_bottom: 1440,
            top_exact: false,
            bottom_exact: false,
            gutter: 0,
            mirror_margins: false,
            default_tab: 720,
            widow_control: true,
            print_hidden: false,
            even_odd_headers: false,
            footnote_position: FootnotePosition::BottomOfPage,
            footnote_restart_each_page: false,
            footnote_start: 1,
            track_changes: false,
            live_toc: None,
        }
    }
}

impl DocumentProps {
    /// Width available for text between the margins.
    pub fn text_width(&self) -> Twips {
        self.page_width
            .saturating_sub(self.margin_left)
            .saturating_sub(self.margin_right)
            .saturating_sub(self.gutter)
    }
}

/// File > Summary Info.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SummaryInfo {
    pub title: String,
    pub subject: String,
    pub author: String,
    pub version: String,
    pub keywords: String,
}

/// A run of paragraphs that changed: `removed` paragraphs starting at
/// `first` were replaced by `inserted` new ones. The layout cache replays
/// these to stay in step with the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub story: StoryId,
    pub first: usize,
    pub removed: usize,
    pub inserted: usize,
}

/// Copied or cut content, ready to paste.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fragment {
    pub paras: Vec<Paragraph>,
    /// True when the last paragraph's mark is part of the fragment. When
    /// false, the last paragraph is a piece of text that joins whatever it
    /// is pasted into.
    pub ends_with_mark: bool,
    /// The pictures its object characters name, by their id in the
    /// document they came from.
    pub objects: Vec<(u32, Object)>,
    /// The footnotes and annotations its references name.
    pub notes: Vec<crate::notes::NoteCopy>,
}

impl Fragment {
    /// The text, with tracked changes taken as accepted.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        let paras = crate::tracking::accepted(&self.paras);
        for (i, p) in paras.iter().enumerate() {
            out.push_str(&p.text);
            if i + 1 < paras.len() || self.ends_with_mark {
                // A table copies as tab-separated rows, the way a
                // spreadsheet takes it; a second paragraph in a cell
                // gives way to a space so the row stays on one line.
                match p.mark {
                    // A table in a cell reads as part of the cell's text.
                    Mark::CellEnd | Mark::RowEnd if p.pap.nest > 0 => out.push(' '),
                    Mark::CellEnd => {
                        if paras.get(i + 1).is_none_or(|n| n.mark != Mark::RowEnd) {
                            out.push('\t');
                        }
                    }
                    Mark::RowEnd => out.push('\n'),
                    _ if p.pap.in_table => out.push(' '),
                    _ => out.push('\n'),
                }
            }
        }
        out
    }
}

#[derive(Clone, Debug)]
struct Record {
    story: StoryId,
    first: usize,
    old: Vec<Paragraph>,
    new_len: usize,
    /// The style sheet as it was, when the step changed it.
    sheet: Option<Box<StyleSheet>>,
    /// The last section's settings as they were, when the step changed
    /// them.
    section: Option<Box<SectionProps>>,
    /// Page, margin, and document-wide settings as they were.
    props: Option<Box<DocumentProps>>,
    /// Summary metadata as it was.
    summary: Option<Box<SummaryInfo>>,
    /// For a header or footer: the section whose it is.
    hf_section: usize,
}

#[derive(Clone, Debug)]
struct Step {
    label: &'static str,
    records: Vec<Record>,
    before: Selection,
    after: Selection,
    /// Identifies the document state this step produces, for "modified".
    id: u64,
}

#[derive(Clone, Debug, Default)]
struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    open: Option<Step>,
    /// The top undo step may be extended by more typing.
    can_coalesce: bool,
    next_id: u64,
    /// State before the oldest retained undo step. This stops a document
    /// whose early history was trimmed from looking like a new empty file
    /// after every remaining step is undone.
    base_id: u64,
}

pub const TYPING: &str = "Typing";

const MIN_UNDO_STEPS: usize = 25;
const MAX_UNDO_STEPS: usize = 100;
const MAX_UNDO_BYTES: usize = 32 * 1024 * 1024;

fn story_owned_bytes(story: &Story) -> usize {
    story.paras.capacity() * std::mem::size_of::<Paragraph>()
        + story.paras.iter().map(paragraph_owned_bytes).sum::<usize>()
}

fn section_owned_bytes(section: &SectionProps) -> usize {
    section.headers.as_ref().map_or(0, |headers| {
        headers.capacity() * std::mem::size_of::<Story>()
            + headers.iter().map(story_owned_bytes).sum::<usize>()
    })
}

fn paragraph_owned_bytes(p: &Paragraph) -> usize {
    p.text.capacity()
        + p.runs.capacity() * std::mem::size_of::<crate::para::Run>()
        + p.pap.tabs.capacity() * std::mem::size_of::<crate::props::TabStop>()
        + p.row.as_ref().map_or(0, |row| {
            std::mem::size_of::<crate::props::RowProps>()
                + row.cells.capacity() * std::mem::size_of::<crate::props::CellProps>()
        })
        + p.section.as_ref().map_or(0, |section| {
            std::mem::size_of::<SectionProps>() + section_owned_bytes(section)
        })
}

fn step_owned_bytes(step: &Step) -> usize {
    step.records.capacity() * std::mem::size_of::<Record>()
        + step
            .records
            .iter()
            .map(|record| {
                record.old.capacity() * std::mem::size_of::<Paragraph>()
                    + record.old.iter().map(paragraph_owned_bytes).sum::<usize>()
                    + record.sheet.as_ref().map_or(0, |sheet| {
                        std::mem::size_of::<StyleSheet>()
                            + sheet.styles.capacity() * std::mem::size_of::<crate::styles::Style>()
                            + sheet
                                .styles
                                .iter()
                                .map(|style| {
                                    style.name.capacity()
                                        + style.pap.tabs.capacity()
                                            * std::mem::size_of::<crate::props::TabStop>()
                                })
                                .sum::<usize>()
                    })
                    + record.section.as_ref().map_or(0, |section| {
                        std::mem::size_of::<SectionProps>() + section_owned_bytes(section)
                    })
                    + record.summary.as_ref().map_or(0, |summary| {
                        summary.title.capacity()
                            + summary.subject.capacity()
                            + summary.author.capacity()
                            + summary.version.capacity()
                            + summary.keywords.capacity()
                    })
            })
            .sum::<usize>()
}

fn visit_object_ids(paras: &[Paragraph], ids: &mut std::collections::BTreeSet<u32>) {
    for p in paras {
        for (start, end, chp) in p.run_bounds() {
            if p.text[start..end].contains(chars::OBJECT) && chp.object != 0 {
                ids.insert(chp.object);
            }
        }
        if let Some(section) = &p.section {
            visit_section_object_ids(section, ids);
        }
    }
}

fn visit_section_object_ids(section: &SectionProps, ids: &mut std::collections::BTreeSet<u32>) {
    if let Some(headers) = &section.headers {
        for story in headers.iter() {
            visit_object_ids(&story.paras, ids);
        }
    }
}

fn visit_step_object_ids(step: &Step, ids: &mut std::collections::BTreeSet<u32>) {
    for record in &step.records {
        visit_object_ids(&record.old, ids);
        if let Some(section) = &record.section {
            visit_section_object_ids(section, ids);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    pub main: Story,
    /// Font names; `CharProps::font` indexes this.
    pub fonts: Vec<String>,
    pub styles: StyleSheet,
    pub props: DocumentProps,
    pub section: SectionProps,
    /// The first section's headers and footers, in `StoryId::HEADERS`
    /// order (later sections have their own, or share; see
    /// `SectionProps::headers`). An unused one is a single empty paragraph.
    pub headers: Vec<Story>,
    /// The section whose headers and footers `StoryId::Header` and the
    /// rest stand for: the one the header pane shows. Not saved.
    pub header_section: usize,
    pub summary: SummaryInfo,
    /// Pictures; `CharProps::object` is an index into this plus one.
    /// Entries are only ever added.
    pub objects: Vec<Object>,
    /// The footnotes' text (see `notes`).
    pub notes: Story,
    /// Footnotes with a mark of their own instead of a number, by id.
    pub note_marks: std::collections::HashMap<u32, String>,
    /// The annotations' text, and who wrote each, by id.
    pub annotations: Story,
    pub annotation_info: std::collections::HashMap<u32, crate::notes::AnnotationInfo>,
    /// The text of notes whose reference was deleted, by id, in case it
    /// is pasted back.
    pub(crate) note_bank: std::collections::HashMap<u32, Vec<Paragraph>>,
    pub(crate) next_note: u32,
    /// Who is editing, for tracked changes. Empty means the app's choice
    /// (`revision::set_author_source`: Preferences' Your Name). Not saved.
    pub author: String,
    history: History,
    changes: Vec<Change>,
    saved_state: u64,
}

impl Default for Document {
    fn default() -> Self {
        Document::new()
    }
}

/// An empty header or footer in the "header" or "footer" style, as a new
/// document has and as one a file leaves out reads.
pub fn blank_header(styles: &StyleSheet, id: StoryId) -> Story {
    let name = if id.is_footer() { "footer" } else { "header" };
    let st = styles.get(styles.find(name).unwrap_or(0));
    Story::new(Paragraph::new(st.pap.clone(), st.chp))
}

impl Document {
    /// A new, empty document: one Normal paragraph in New York 12 pt, as
    /// File > New makes in the original.
    pub fn new() -> Self {
        Document::with_default_font("New York", 24)
    }

    pub fn with_default_font(font: &str, size: u16) -> Self {
        let mut fonts = vec![font.to_string()];
        let styles = StyleSheet::standard(0, size, &mut |name| intern(&mut fonts, name));
        let normal = styles.get(0);
        let first = Paragraph::new(normal.pap.clone(), normal.chp);
        let headers = StoryId::HEADERS
            .iter()
            .map(|&id| blank_header(&styles, id))
            .collect();
        let notes = Story::new({
            let text = styles.find("footnote text").unwrap_or(0);
            let st = styles.get(text);
            let mut p = Paragraph::new(st.pap.clone(), st.chp);
            p.pap.style = text;
            p
        });
        let annotations = Story::new(Paragraph::new(normal.pap.clone(), normal.chp));
        Document {
            main: Story::new(first),
            fonts,
            styles,
            props: DocumentProps::default(),
            section: SectionProps::default(),
            headers,
            header_section: 0,
            summary: SummaryInfo::default(),
            objects: Vec::new(),
            notes,
            note_marks: Default::default(),
            annotations,
            annotation_info: Default::default(),
            note_bank: Default::default(),
            next_note: 1,
            author: String::new(),
            history: History::default(),
            changes: Vec::new(),
            saved_state: 0,
        }
    }

    pub fn story(&self, id: StoryId) -> &Story {
        match id {
            StoryId::Main => &self.main,
            StoryId::Notes => &self.notes,
            StoryId::Annotations => &self.annotations,
            _ => self.header_story(self.header_section, id),
        }
    }

    /// A header or footer story to change, for `header_section`. A section
    /// sharing the one before's gets its own copy first, as typing into a
    /// Same As Previous header does in the original.
    fn story_mut(&mut self, id: StoryId) -> &mut Story {
        match id {
            StoryId::Main => &mut self.main,
            StoryId::Notes => &mut self.notes,
            StoryId::Annotations => &mut self.annotations,
            _ => {
                let k = self.header_section.min(self.sections().len() - 1);
                if k == 0 {
                    return &mut self.headers[id.header_index()];
                }
                if self.nth_section(k).headers.is_none() {
                    let own = self.headers_of(k).to_vec();
                    self.nth_section_mut(k).headers = Some(std::sync::Arc::new(own));
                }
                let set = self.nth_section_mut(k).headers.as_mut().unwrap();
                &mut std::sync::Arc::make_mut(set)[id.header_index()]
            }
        }
    }

    /// Section `k`'s settings.
    pub fn nth_section(&self, k: usize) -> &SectionProps {
        let paras = &self.main.paras;
        let mut s = 0;
        for (i, p) in paras.iter().enumerate() {
            if p.mark == Mark::SectionBreak && i + 1 < paras.len() {
                if s == k {
                    return p.section.as_deref().unwrap_or(&self.section);
                }
                s += 1;
            }
        }
        &self.section
    }

    /// Section `k`'s settings to change in place, outside undo: for header
    /// stories, whose changes the header pane keeps.
    fn nth_section_mut(&mut self, k: usize) -> &mut SectionProps {
        let ranges = self.sections();
        match ranges.get(k) {
            Some(r) if k + 1 < ranges.len() => {
                let end = *r.end();
                let fallback = self.section.clone();
                self.main.paras[end]
                    .section
                    .get_or_insert_with(|| Box::new(fallback))
            }
            _ => &mut self.section,
        }
    }

    /// The section whose headers and footers section `k` shows: itself if
    /// it has its own, otherwise the nearest one before that has.
    pub fn header_source(&self, k: usize) -> usize {
        let paras = &self.main.paras;
        let mut source = 0;
        let mut s = 0;
        for (i, p) in paras.iter().enumerate() {
            if s > k {
                return source;
            }
            if p.mark == Mark::SectionBreak && i + 1 < paras.len() {
                let props = p.section.as_deref().unwrap_or(&self.section);
                if s > 0 && props.headers.is_some() {
                    source = s;
                }
                s += 1;
            }
        }
        if s > 0 && s <= k && self.section.headers.is_some() {
            source = s;
        }
        source
    }

    /// For each section, the section whose headers and footers it shows and
    /// those headers and footers: `header_source` and `headers_of` for every
    /// section in one pass over the text, for loops that ask about each page.
    pub fn header_sets(&self) -> Vec<(usize, &[Story])> {
        let paras = &self.main.paras;
        let breaks = paras
            .iter()
            .enumerate()
            .filter(|(i, p)| p.mark == Mark::SectionBreak && i + 1 < paras.len())
            .map(|(_, p)| p.section.as_deref().unwrap_or(&self.section));
        let mut current: (usize, &[Story]) = (0, &self.headers);
        let mut out = Vec::new();
        for (s, props) in breaks.chain(std::iter::once(&self.section)).enumerate() {
            if s > 0
                && let Some(own) = props.headers.as_deref()
            {
                current = (s, own.as_slice());
            }
            out.push(current);
        }
        out
    }

    /// The headers and footers section `k` shows.
    pub fn headers_of(&self, k: usize) -> &[Story] {
        match self.header_source(k) {
            0 => &self.headers,
            s => self
                .nth_section(s)
                .headers
                .as_deref()
                .map_or(&self.headers, |v| v),
        }
    }

    /// Header or footer `id` as section `k` shows it.
    pub fn header_story(&self, k: usize, id: StoryId) -> &Story {
        &self.headers_of(k)[id.header_index()]
    }

    /// Whether section `k` has headers and footers of its own (the first
    /// always has).
    pub fn has_own_headers(&self, k: usize) -> bool {
        k == 0 || self.nth_section(k).headers.is_some()
    }

    /// Same As Previous: section `k` gives up its own headers and footers
    /// and shows the section before's, as one undoable change.
    pub fn link_headers(&mut self, k: usize) {
        if k == 0 || !self.has_own_headers(k) {
            return;
        }
        let at = *self.sections()[k.min(self.sections().len() - 1)].start();
        self.format_sections(at, at, |s| s.headers = None);
        for id in StoryId::HEADERS {
            let n = self.header_story(k, id).paras.len();
            self.changes.push(Change {
                story: id,
                first: 0,
                removed: n,
                inserted: n,
            });
        }
    }

    /// Copy a story from another document of the same text (the header
    /// pane's copy), mapping its fonts by name into this document's table
    /// and bringing over the pictures it shows from that document's
    /// `objects`.
    pub fn import_story(
        &mut self,
        id: StoryId,
        story: &Story,
        fonts: &[String],
        objects: &[Object],
    ) {
        let mut story = story.clone();
        for p in &mut story.paras {
            let mut map = |c: &mut CharProps| {
                if let Some(name) = fonts.get(c.font as usize) {
                    c.font = intern(&mut self.fonts, name);
                }
            };
            let len = p.len();
            p.format_range(0, len, &mut map);
            map(&mut p.mark_chp);
            // A picture's number is its place in the other document's
            // list; give it one here (the same if it is already here). Only
            // pictures have one: on a footnote or comment mark the same
            // field is the note's id, and stays as it is.
            let pictures: Vec<(usize, usize)> = p
                .run_bounds()
                .filter(|&(a, b, _)| p.text[a..b].contains(crate::para::chars::OBJECT))
                .map(|(a, b, _)| (a, b))
                .collect();
            for (a, b) in pictures {
                p.format_range(a, b, &mut |c: &mut CharProps| {
                    if c.object != 0
                        && let Some(o) = objects.get(c.object as usize - 1)
                    {
                        c.object = self.add_object(o.clone());
                    }
                });
            }
        }
        self.set_story(id, story);
    }

    /// A fragment from another document (a glossary entry) with its fonts
    /// and styles, given by name, matched to this document's. Fonts it
    /// lacks are added; styles it lacks become Normal.
    pub fn import_fragment(
        &mut self,
        frag: &Fragment,
        fonts: &[String],
        styles: &[String],
    ) -> Fragment {
        let mut out = frag.clone();
        for p in &mut out.paras {
            let mut map = |c: &mut CharProps| {
                if let Some(name) = fonts.get(c.font as usize) {
                    c.font = intern(&mut self.fonts, name);
                }
            };
            let len = p.len();
            p.format_range(0, len, &mut map);
            map(&mut p.mark_chp);
            p.pap.style = styles
                .get(p.pap.style as usize)
                .and_then(|n| self.styles.find(n))
                .unwrap_or(0);
        }
        out
    }

    /// Whether a header or footer has nothing in it.
    pub fn story_is_empty(&self, id: StoryId) -> bool {
        let s = self.story(id);
        s.paras.len() == 1 && s.paras[0].text.is_empty()
    }

    /// Replace a header or footer wholesale, as the header pane does after
    /// editing its own copy. Not undoable here; the pane keeps its own undo.
    pub fn set_story(&mut self, id: StoryId, story: Story) {
        if *self.story(id) == story || story.paras.is_empty() {
            return;
        }
        let removed = self.story(id).paras.len();
        let inserted = story.paras.len();
        *self.story_mut(id) = story;
        self.changes.push(Change {
            story: id,
            first: 0,
            removed,
            inserted,
        });
        self.touch();
    }

    /// Font id for `name`, adding it to the font table if new.
    pub fn font_id(&mut self, name: &str) -> FontId {
        intern(&mut self.fonts, name)
    }

    pub fn font_name(&self, id: FontId) -> &str {
        self.fonts
            .get(id as usize)
            .map(String::as_str)
            .unwrap_or("")
    }

    /// Paragraph changes since the last call.
    pub fn take_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }

    // ----- undo -----

    /// Start an undoable step. Every edit until `commit` undoes as one.
    pub fn begin(&mut self, label: &'static str, sel: Selection) {
        if self.history.open.is_some() {
            return;
        }
        self.history.can_coalesce = false;
        self.history.open = Some(Step {
            label,
            records: Vec::new(),
            before: sel,
            after: sel,
            id: 0,
        });
    }

    /// Start a typing step, joining the previous one if the user has kept
    /// typing at the same place.
    pub fn begin_typing(&mut self, sel: Selection) {
        if self.history.open.is_some() {
            return;
        }
        let joins = self.history.can_coalesce
            && self
                .history
                .undo
                .last()
                .is_some_and(|s| s.label == TYPING && s.after == sel);
        if joins {
            self.history.open = self.history.undo.pop();
        } else {
            self.begin(TYPING, sel);
        }
    }

    /// Finish the open step. Steps that changed nothing are dropped.
    pub fn commit(&mut self, sel: Selection) {
        let Some(mut step) = self.history.open.take() else {
            return;
        };
        if step.records.is_empty() {
            return;
        }
        step.after = sel;
        let continuing_typing = step.id != 0;
        if step.id == 0 {
            self.history.next_id += 1;
            step.id = self.history.next_id;
        }
        self.history.can_coalesce = step.label == TYPING;
        self.history.undo.push(step);
        let reclaim_redo_objects = self.history.redo.iter().any(|step| {
            let mut ids = std::collections::BTreeSet::new();
            visit_step_object_ids(step, &mut ids);
            !ids.is_empty()
        });
        self.history.redo.clear();
        if !continuing_typing || reclaim_redo_objects {
            self.trim_history(
                MIN_UNDO_STEPS,
                MAX_UNDO_STEPS,
                MAX_UNDO_BYTES,
                reclaim_redo_objects,
            );
        }
    }

    /// Bound history by both action count and owned memory. The newest
    /// `minimum` steps are unconditional, so even large edits remain useful;
    /// after that the byte budget decides how far back Undo can reach.
    fn trim_history(
        &mut self,
        minimum: usize,
        maximum: usize,
        byte_budget: usize,
        mut reclaim_objects: bool,
    ) {
        while self.history.undo.len() > maximum.max(minimum) {
            let discarded = self.history.undo.remove(0);
            let mut ids = std::collections::BTreeSet::new();
            visit_step_object_ids(&discarded, &mut ids);
            reclaim_objects |= !ids.is_empty();
            self.history.base_id = discarded.id;
        }
        while self.history.undo.len() > minimum && self.undo_owned_bytes() > byte_budget {
            let discarded = self.history.undo.remove(0);
            let mut ids = std::collections::BTreeSet::new();
            visit_step_object_ids(&discarded, &mut ids);
            reclaim_objects |= !ids.is_empty();
            self.history.base_id = discarded.id;
        }
        if reclaim_objects {
            self.reclaim_unreachable_objects();
        }
    }

    fn live_object_ids(&self) -> std::collections::BTreeSet<u32> {
        let mut ids = std::collections::BTreeSet::new();
        visit_object_ids(&self.main.paras, &mut ids);
        visit_object_ids(&self.notes.paras, &mut ids);
        visit_object_ids(&self.annotations.paras, &mut ids);
        for story in &self.headers {
            visit_object_ids(&story.paras, &mut ids);
        }
        visit_section_object_ids(&self.section, &mut ids);
        for paras in self.note_bank.values() {
            visit_object_ids(paras, &mut ids);
        }
        ids
    }

    fn history_object_ids(&self) -> std::collections::BTreeSet<u32> {
        let mut ids = std::collections::BTreeSet::new();
        for step in self.history.undo.iter().chain(&self.history.redo) {
            visit_step_object_ids(step, &mut ids);
        }
        if let Some(step) = &self.history.open {
            visit_step_object_ids(step, &mut ids);
        }
        ids
    }

    fn undo_owned_bytes(&self) -> usize {
        let mut total = self
            .history
            .undo
            .iter()
            .map(step_owned_bytes)
            .fold(0usize, usize::saturating_add);
        let history_ids = self.history_object_ids();
        let mut allocated = std::collections::HashSet::<(usize, usize)>::new();
        let mut remember = |picture: &Picture| {
            for bytes in std::iter::once(&picture.data).chain(picture.preview.iter()) {
                let allocation = (bytes.as_ptr() as usize, bytes.len());
                if allocated.insert(allocation) {
                    total = total.saturating_add(bytes.len());
                }
            }
        };
        for id in history_ids {
            if let Some(Object::Picture(picture)) = self.object(id) {
                remember(picture);
            }
        }
        total
    }

    fn reclaim_unreachable_objects(&mut self) {
        let mut keep = self.live_object_ids();
        keep.extend(self.history_object_ids());
        for (index, object) in self.objects.iter_mut().enumerate() {
            if !keep.contains(&(index as u32 + 1)) {
                *object = Object::Vacant;
            }
        }
    }

    /// Stop the next typing from joining the last typing step, for example
    /// because the insertion point moved.
    pub fn break_typing(&mut self) {
        self.history.can_coalesce = false;
    }

    pub fn undo_label(&self) -> Option<&'static str> {
        self.history.undo.last().map(|s| s.label)
    }

    pub fn redo_label(&self) -> Option<&'static str> {
        self.history.redo.last().map(|s| s.label)
    }

    /// Reopen the last undoable step, so edits made now undo with it. Used
    /// for changes that follow from an edit, like a live table of contents
    /// catching up. Starts a new step labeled `label` when there is none.
    pub fn continue_last(&mut self, label: &'static str, sel: Selection) {
        if self.history.open.is_some() {
            return;
        }
        match self.history.undo.pop() {
            Some(step) => self.history.open = Some(step),
            None => self.begin(label, sel),
        }
    }

    /// Undo the last step and return the selection from before it.
    pub fn undo(&mut self) -> Option<Selection> {
        self.commit_open();
        let step = self.history.undo.pop()?;
        let sel = step.before;
        let inverse = self.apply_inverse(step);
        self.history.redo.push(inverse);
        self.history.can_coalesce = false;
        Some(sel)
    }

    /// Redo the last undone step and return the selection after it.
    pub fn redo(&mut self) -> Option<Selection> {
        self.commit_open();
        let step = self.history.redo.pop()?;
        let inverse = self.apply_inverse(step);
        let sel = inverse.after;
        self.history.undo.push(inverse);
        self.history.can_coalesce = false;
        Some(sel)
    }

    fn commit_open(&mut self) {
        if let Some(step) = &self.history.open {
            let sel = step.after;
            self.commit(sel);
        }
    }

    /// Apply a step's records backwards, returning the step that reverses
    /// it. Undo turns an undo step into a redo step and vice versa.
    fn apply_inverse(&mut self, step: Step) -> Step {
        let mut records = Vec::with_capacity(step.records.len());
        for r in step.records.into_iter().rev() {
            if let Some(sheet) = r.sheet {
                let now = std::mem::replace(&mut self.styles, *sheet);
                records.push(Record {
                    story: r.story,
                    first: 0,
                    old: Vec::new(),
                    new_len: 0,
                    sheet: Some(Box::new(now)),
                    section: None,
                    props: None,
                    summary: None,
                    hf_section: 0,
                });
                continue;
            }
            if let Some(sec) = r.section {
                let now = std::mem::replace(&mut self.section, *sec);
                records.push(Record {
                    story: r.story,
                    first: 0,
                    old: Vec::new(),
                    new_len: 0,
                    sheet: None,
                    section: Some(Box::new(now)),
                    props: None,
                    summary: None,
                    hf_section: 0,
                });
                continue;
            }
            if let Some(props) = r.props {
                let now = std::mem::replace(&mut self.props, *props);
                records.push(Record {
                    story: r.story,
                    first: 0,
                    old: Vec::new(),
                    new_len: 0,
                    sheet: None,
                    section: None,
                    props: Some(Box::new(now)),
                    summary: None,
                    hf_section: 0,
                });
                continue;
            }
            if let Some(summary) = r.summary {
                let now = std::mem::replace(&mut self.summary, *summary);
                records.push(Record {
                    story: r.story,
                    first: 0,
                    old: Vec::new(),
                    new_len: 0,
                    sheet: None,
                    section: None,
                    props: None,
                    summary: Some(Box::new(now)),
                    hf_section: 0,
                });
                continue;
            }
            // A header or footer goes back into the section it came from.
            if r.story != StoryId::Main && r.story.header_index_checked().is_some() {
                self.header_section = r.hf_section;
            }
            let story = self.story_mut(r.story);
            let new_len = r.old.len();
            let old: Vec<Paragraph> = story
                .paras
                .splice(r.first..r.first + r.new_len, r.old)
                .collect();
            self.changes.push(Change {
                story: r.story,
                first: r.first,
                removed: old.len(),
                inserted: new_len,
            });
            records.push(Record {
                story: r.story,
                first: r.first,
                old,
                new_len,
                sheet: None,
                section: None,
                props: None,
                summary: None,
                hf_section: r.hf_section,
            });
        }
        Step {
            label: step.label,
            records,
            before: step.before,
            after: step.after,
            id: step.id,
        }
    }

    /// Drop all undo history and count the document as unmodified. Used
    /// after loading a file.
    pub fn forget_history(&mut self) {
        self.history = History::default();
        self.saved_state = 0;
        self.changes.clear();
        self.reclaim_unreachable_objects();
    }

    /// Count the document as changed by something outside the undo
    /// history, such as its summary info.
    pub fn touch(&mut self) {
        self.saved_state = u64::MAX - 1;
    }

    /// True when the document differs from the last `mark_saved`.
    pub fn is_modified(&self) -> bool {
        self.state_id() != self.saved_state
    }

    pub fn mark_saved(&mut self) {
        self.commit_open();
        self.saved_state = self.state_id();
    }

    fn state_id(&self) -> u64 {
        let open = self
            .history
            .open
            .as_ref()
            .is_some_and(|s| !s.records.is_empty());
        if open {
            return u64::MAX;
        }
        self.history
            .undo
            .last()
            .map_or(self.history.base_id, |s| s.id)
    }

    /// Replace paragraphs `first..first + remove` of a story with `new`.
    /// Every edit ends up here.
    pub(crate) fn splice(
        &mut self,
        story: StoryId,
        first: usize,
        remove: usize,
        new: Vec<Paragraph>,
    ) {
        assert!(
            !new.is_empty() || remove < self.story(story).paras.len(),
            "a story needs a paragraph"
        );
        let inserted = new.len();
        let changed_kinds: Vec<crate::notes::NoteKind> = if story == StoryId::Main {
            crate::notes::NoteKind::ALL
                .into_iter()
                .filter(|&k| {
                    let mut a = crate::notes::ref_ids(&self.main.paras[first..first + remove], k);
                    let mut b = crate::notes::ref_ids(&new, k);
                    a.sort_unstable();
                    b.sort_unstable();
                    a != b
                })
                .collect()
        } else {
            Vec::new()
        };
        let s = self.story_mut(story);
        let old: Vec<Paragraph> = s.paras.splice(first..first + remove, new).collect();
        self.changes.push(Change {
            story,
            first,
            removed: remove,
            inserted,
        });
        if self.history.open.is_none() {
            self.begin("Edit", Selection::default());
        }
        let step = self.history.open.as_mut().unwrap();
        let rec = Record {
            story,
            first,
            old,
            new_len: inserted,
            sheet: None,
            section: None,
            props: None,
            summary: None,
            hf_section: self.header_section,
        };
        // If this edit only touches paragraphs the step already replaced,
        // the step's saved originals still cover it: just track the size.
        if let Some(last) = step.records.last_mut()
            && last.sheet.is_none()
            && last.section.is_none()
            && last.props.is_none()
            && last.summary.is_none()
            && last.story == rec.story
            && last.hf_section == rec.hf_section
            && rec.first >= last.first
            && rec.first + rec.old.len() <= last.first + last.new_len
        {
            last.new_len = last.new_len - rec.old.len() + rec.new_len;
        } else {
            step.records.push(rec);
        }
        // References came or went: bring the notes into line.
        for kind in changed_kinds {
            self.reconcile(kind);
        }
    }

    // ----- edits -----

    /// Insert text that contains no paragraph breaks at `at`, formatted
    /// `chp`. Returns the position after it.
    pub fn insert_text(
        &mut self,
        story: StoryId,
        at: Position,
        text: &str,
        chp: CharProps,
    ) -> Position {
        if text.is_empty() {
            return at;
        }
        debug_assert!(
            !text.contains(['\n', '\r']),
            "use insert_plain for paragraphs"
        );
        let chp = CharProps {
            rev: self.new_text_rev(),
            ..chp
        };
        let mut p = self.story(story).paras[at.para].clone();
        p.insert(at.offset, text, chp);
        self.splice(story, at.para, 1, vec![p]);
        Position::new(at.para, at.offset + text.len())
    }

    /// Insert text where `\n` (or `\r\n`, `\r`) starts a new paragraph.
    /// New paragraphs copy the format of the paragraph at `at`.
    pub fn insert_plain(
        &mut self,
        story: StoryId,
        at: Position,
        text: &str,
        chp: CharProps,
    ) -> Position {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let pap = self.story(story).paras[at.para].pap.clone();
        let mut lines: Vec<&str> = normalized.split('\n').collect();
        // "a\n" is one whole paragraph; "a\nb" is a paragraph plus a piece.
        let ends_with_mark = lines.len() > 1 && lines.last() == Some(&"");
        if ends_with_mark {
            lines.pop();
        }
        let paras = lines
            .iter()
            .map(|line| Paragraph::with_text(line, chp, pap.clone()))
            .collect();
        self.insert_fragment(
            story,
            at,
            &Fragment {
                paras,
                ends_with_mark,
                objects: Vec::new(),
                notes: Vec::new(),
            },
        )
    }

    /// Return: split the paragraph at `at`. At the end of a paragraph whose
    /// style names a different next style, the new paragraph gets that
    /// style. Returns the start of the new paragraph.
    pub fn split_paragraph(&mut self, story: StoryId, at: Position) -> Position {
        let mut head = self.story(story).paras[at.para].clone();
        let at_end = at.offset == head.len();
        let mut tail = head.split_off(at.offset);
        // The first half's mark is the new one.
        head.mark_chp.rev = self.new_text_rev();
        if at_end {
            let style = self.styles.get(head.pap.style);
            if style.next != head.pap.style {
                let next = self.styles.get(style.next);
                tail.pap = next.pap.clone();
                tail.mark_chp = next.chp;
                // Starting a new style does not take the paragraph out of
                // its table cell.
                tail.pap.in_table = head.pap.in_table;
                tail.pap.nest = head.pap.nest;
            }
        }
        self.splice(story, at.para, 1, vec![head, tail]);
        Position::new(at.para + 1, 0)
    }

    /// Delete `start..end`. Deleting a paragraph mark joins the text before
    /// it to the following paragraph, which keeps its own format: the
    /// format lives in the mark, as in the original. The story's last mark is never
    /// deleted. Returns `start`.
    pub fn delete(&mut self, story: StoryId, start: Position, end: Position) -> Position {
        let (start, end) = (start.min(end), start.max(end));
        self.delete_and_end(story, start, end);
        start
    }

    /// Delete `start..end` and return where its end is now: the start,
    /// unless Track Changes kept the deleted text in place.
    pub fn delete_and_end(&mut self, story: StoryId, start: Position, end: Position) -> Position {
        let (start, end) = (start.min(end), start.max(end));
        if start == end {
            return start;
        }
        if let Some(by) = self.tracking() {
            return self.delete_tracked(story, start, end, &by);
        }
        let paras = &self.story(story).paras;
        if start.para == end.para {
            let mut p = paras[start.para].clone();
            p.remove(start.offset, end.offset);
            self.splice(story, start.para, 1, vec![p]);
            return start;
        }
        let last = &paras[end.para];
        let mut joined = paras[start.para].slice(0, start.offset);
        joined.append(&last.slice(end.offset, last.len()));
        joined.pap = last.pap.clone();
        joined.mark = last.mark;
        joined.mark_chp = last.mark_chp;
        joined.row = last.row.clone();
        joined.section = last.section.clone();
        self.splice(story, start.para, end.para - start.para + 1, vec![joined]);
        start
    }

    /// Copy `start..end` as a fragment.
    /// Attach the pictures a fragment's paragraphs name.
    pub(crate) fn with_objects(&self, mut frag: Fragment) -> Fragment {
        let mut ids: Vec<u32> = frag
            .paras
            .iter()
            .flat_map(|p| {
                p.run_bounds()
                    .filter(|(s, e, _)| p.text[*s..*e].contains(chars::OBJECT))
                    .map(|(_, _, c)| c.object)
            })
            .filter(|&id| id != 0)
            .collect();
        ids.sort();
        ids.dedup();
        frag.objects = ids
            .into_iter()
            .filter_map(|id| Some((id, self.object(id)?.clone())))
            .collect();
        frag.notes = self.notes_for(&frag.paras);
        frag
    }

    pub fn copy(&self, story: StoryId, start: Position, end: Position) -> Fragment {
        let (start, end) = (start.min(end), start.max(end));
        let paras = &self.story(story).paras;
        if start.para == end.para {
            let p = &paras[start.para];
            return self.with_objects(Fragment {
                paras: vec![p.slice(start.offset, end.offset)],
                ends_with_mark: false,
                objects: Vec::new(),
                notes: Vec::new(),
            });
        }
        let mut out = vec![paras[start.para].slice(start.offset, paras[start.para].len())];
        out.extend(paras[start.para + 1..end.para].iter().cloned());
        let ends_with_mark = end.offset == 0;
        if !ends_with_mark {
            out.push(paras[end.para].slice(0, end.offset));
        }
        self.with_objects(Fragment {
            paras: out,
            ends_with_mark,
            objects: Vec::new(),
            notes: Vec::new(),
        })
    }

    /// Paste a fragment at `at`. Returns the position after it.
    pub fn insert_fragment(&mut self, story: StoryId, at: Position, frag: &Fragment) -> Position {
        if frag.paras.is_empty() {
            return at;
        }
        // Deleted text stays behind, and what is pasted is new text.
        let fresh = Fragment {
            paras: Document::fresh_paste(&frag.paras, self.new_text_rev()),
            ..frag.clone()
        };
        let frag = &fresh;
        // Pictures join this document's objects, under new ids if need be,
        // and pasted footnotes become new notes.
        let remapped;
        let frag = if frag.objects.is_empty() && frag.notes.is_empty() {
            frag
        } else {
            let map: Vec<(u32, u32)> = frag
                .objects
                .iter()
                .map(|(id, o)| (*id, self.add_object(o.clone())))
                .collect();
            let mut f = frag.clone();
            for p in &mut f.paras {
                let spans: Vec<(usize, usize)> = p
                    .run_bounds()
                    .filter(|(s, e, _)| p.text[*s..*e].contains(chars::OBJECT))
                    .map(|(s, e, _)| (s, e))
                    .collect();
                for (s, e) in spans {
                    p.format_range(s, e, &mut |c| {
                        if let Some((_, new)) = map.iter().find(|(old, _)| *old == c.object) {
                            c.object = *new;
                        }
                    });
                }
            }
            if story == StoryId::Main {
                let notes = std::mem::take(&mut f.notes);
                self.adopt_notes(&mut f.paras, &notes);
            }
            f.objects.clear();
            remapped = f;
            &remapped
        };
        let mut target = self.story(story).paras[at.para].clone();
        // Tables arrive whole or as text, never as broken rows. Inside a
        // cell, a whole table arrives as a table in that cell.
        let depth = target.pap.level();
        let fitted;
        let frag = if frag.paras.iter().any(|p| p.pap.in_table) || depth > 0 {
            let mut f = frag.clone();
            crate::table::fit_pasted(&mut f.paras, depth);
            if f.paras.is_empty() {
                return at;
            }
            fitted = f;
            &fitted
        } else {
            frag
        };
        // A pasted table starts and ends on a paragraph of its own: the
        // text before `at` stays in its paragraph, and the text after it
        // follows the table.
        if frag.paras[0].pap.level() > depth {
            let tail = target.split_off(at.offset);
            let mut out = Vec::with_capacity(frag.paras.len() + 2);
            if at.offset > 0 {
                out.push(target);
            }
            out.extend(frag.paras.iter().cloned());
            let mut tail = tail;
            if !frag.ends_with_mark && !frag.paras.last().is_some_and(|p| p.pap.level() > depth) {
                let mut last = out.pop().unwrap();
                let offset = last.len();
                last.append(&tail);
                last.pap = tail.pap.clone();
                last.mark = tail.mark;
                last.row = tail.row.clone();
                last.section = tail.section.clone();
                last.mark_chp = tail.mark_chp;
                out.push(last);
                let end = Position::new(at.para + out.len() - 1, offset);
                self.splice(story, at.para, 1, out);
                return end;
            }
            tail.pap.set_level(depth);
            out.push(tail);
            let end = Position::new(at.para + out.len() - 1, 0);
            self.splice(story, at.para, 1, out);
            return end;
        }
        if frag.paras.len() == 1 && !frag.ends_with_mark {
            let piece = &frag.paras[0];
            let mut offset = at.offset;
            for (s, e, chp) in piece.run_bounds() {
                target.insert(offset, &piece.text[s..e], *chp);
                offset += e - s;
            }
            self.splice(story, at.para, 1, vec![target]);
            return Position::new(at.para, offset);
        }
        // The text before `at` joins the first pasted paragraph and takes its
        // format, because that paragraph's mark comes with the paste. The
        // text after `at` keeps the target paragraph's format.
        let tail = target.split_off(at.offset);
        let mut first = target;
        first.append(&frag.paras[0]);
        first.pap = frag.paras[0].pap.clone();
        first.mark_chp = frag.paras[0].mark_chp;
        first.mark = Mark::Paragraph;
        // A pasted section break stays one.
        if frag.paras[0].mark == Mark::SectionBreak {
            first.mark = Mark::SectionBreak;
            first.section = frag.paras[0].section.clone();
        }
        let mut out: Vec<Paragraph> = Vec::with_capacity(frag.paras.len() + 1);
        out.push(first);
        out.extend(frag.paras[1..].iter().cloned());
        let end;
        // A pasted table's last row keeps its mark: the text after `at`
        // follows on a paragraph of its own.
        if frag.ends_with_mark || out.last().is_some_and(|p| p.pap.level() > 0) {
            end = Position::new(at.para + out.len(), 0);
            out.push(tail);
        } else {
            let mut last = out.pop().unwrap();
            let offset = last.len();
            last.append(&tail);
            last.pap = tail.pap.clone();
            last.mark = tail.mark;
            last.row = tail.row.clone();
            last.section = tail.section.clone();
            last.mark_chp = tail.mark_chp;
            out.push(last);
            end = Position::new(at.para + out.len() - 1, offset);
        }
        self.splice(story, at.para, 1, out);
        end
    }

    /// The object an object character names.
    pub fn object(&self, id: u32) -> Option<&Object> {
        match self.objects.get((id as usize).checked_sub(1)?)? {
            Object::Vacant => None,
            object => Some(object),
        }
    }

    pub fn picture(&self, id: u32) -> Option<&Picture> {
        match self.object(id)? {
            Object::Picture(p) => Some(p),
            Object::Vacant => None,
        }
    }

    /// A small, history-free document for editing one story in a pane.
    /// Picture bytes remain shared through `Arc`; unrelated body, note and
    /// header text is not copied into the pane merely to supply its fonts and
    /// page settings.
    pub fn copy_story_for_edit(&self, id: StoryId) -> Document {
        let mut out = Document::new();
        out.main = self.story(id).clone();
        out.fonts = self.fonts.clone();
        out.styles = self.styles.clone();
        out.props = self.props.clone();
        out.section = self.section.clone();
        out.section.headers = None;
        out.header_section = self.header_section;
        out.objects = self.objects.clone();
        out.note_marks = self.note_marks.clone();
        out.annotation_info = self.annotation_info.clone();
        out.next_note = self.next_note;
        out.forget_history();
        out
    }

    /// Add an object (or find the same one already there) and return its
    /// id.
    pub fn add_object(&mut self, o: Object) -> u32 {
        if o == Object::Vacant {
            return 0;
        }
        if let Some(i) = self.objects.iter().position(|x| *x == o) {
            return i as u32 + 1;
        }
        if let Some(i) = self.objects.iter().position(|x| *x == Object::Vacant) {
            self.objects[i] = o;
            return i as u32 + 1;
        }
        self.objects.push(o);
        self.objects.len() as u32
    }

    /// A history-free copy containing only objects reachable from the text.
    /// Saving uses this so an image retained solely by Undo is not kept in
    /// the file after its last placement has been deleted.
    pub fn compact_for_save(&self) -> Document {
        fn visit_story(s: &Story, ids: &mut std::collections::BTreeSet<u32>) {
            for p in &s.paras {
                let mut at: usize = 0;
                for r in &p.runs {
                    let end = at.saturating_add(r.len).min(p.text.len());
                    if p.text
                        .get(at..end)
                        .is_some_and(|t| t.contains(chars::OBJECT))
                    {
                        ids.insert(r.chp.object);
                    }
                    at = end;
                }
                if let Some(section) = &p.section
                    && let Some(headers) = &section.headers
                {
                    for h in headers.iter() {
                        visit_story(h, ids);
                    }
                }
            }
        }
        fn map_story(s: &mut Story, map: &[u32]) {
            for p in &mut s.paras {
                let mut at: usize = 0;
                for r in &mut p.runs {
                    let end = at.saturating_add(r.len).min(p.text.len());
                    if p.text
                        .get(at..end)
                        .is_some_and(|t| t.contains(chars::OBJECT))
                    {
                        r.chp.object = map.get(r.chp.object as usize).copied().unwrap_or(0);
                    }
                    at = end;
                }
                if let Some(section) = &mut p.section
                    && let Some(headers) = &mut section.headers
                {
                    for h in std::sync::Arc::make_mut(headers) {
                        map_story(h, map);
                    }
                }
            }
        }

        let mut ids = std::collections::BTreeSet::new();
        visit_story(&self.main, &mut ids);
        visit_story(&self.notes, &mut ids);
        visit_story(&self.annotations, &mut ids);
        for h in &self.headers {
            visit_story(h, &mut ids);
        }
        if let Some(headers) = &self.section.headers {
            for h in headers.iter() {
                visit_story(h, &mut ids);
            }
        }
        ids.remove(&0);

        let mut map = vec![0; self.objects.len() + 1];
        let mut objects = Vec::new();
        for old in ids {
            if let Some(object) = self.object(old) {
                objects.push(object.clone());
                map[old as usize] = objects.len() as u32;
            }
        }
        let mut out = Document {
            main: self.main.clone(),
            fonts: self.fonts.clone(),
            styles: self.styles.clone(),
            props: self.props.clone(),
            section: self.section.clone(),
            headers: self.headers.clone(),
            header_section: 0,
            summary: self.summary.clone(),
            objects,
            notes: self.notes.clone(),
            note_marks: self.note_marks.clone(),
            annotations: self.annotations.clone(),
            annotation_info: self.annotation_info.clone(),
            note_bank: Default::default(),
            next_note: self.next_note,
            author: self.author.clone(),
            history: History::default(),
            changes: Vec::new(),
            saved_state: 0,
        };
        map_story(&mut out.main, &map);
        map_story(&mut out.notes, &map);
        map_story(&mut out.annotations, &map);
        for h in &mut out.headers {
            map_story(h, &map);
        }
        if let Some(headers) = &mut out.section.headers {
            for h in std::sync::Arc::make_mut(headers) {
                map_story(h, &map);
            }
        }
        out
    }

    /// Insert a picture at `at` in the format `chp`. Returns the position
    /// after it.
    pub fn insert_picture(
        &mut self,
        story: StoryId,
        at: Position,
        pic: Picture,
        chp: CharProps,
    ) -> Position {
        let id = self.add_object(Object::Picture(pic));
        let chp = CharProps { object: id, ..chp };
        self.insert_text(story, at, &chars::OBJECT.to_string(), chp)
    }

    /// Replace the picture whose character starts at `at` (after scaling
    /// or cropping it).
    pub fn set_picture(&mut self, story: StoryId, at: Position, pic: Picture) {
        let id = self.add_object(Object::Picture(pic));
        let end = Position::new(at.para, at.offset + chars::OBJECT.len_utf8());
        self.format_chars(story, at, end, |c| c.object = id);
    }

    /// Apply `f` to the character format of `start..end`, including the
    /// marks of paragraphs the range runs past.
    pub fn format_chars(
        &mut self,
        story: StoryId,
        start: Position,
        end: Position,
        mut f: impl FnMut(&mut CharProps),
    ) {
        let (start, end) = (start.min(end), start.max(end));
        if start == end {
            return;
        }
        let by = self.tracking();
        let mut f = |c: &mut CharProps| {
            let before = *c;
            f(c);
            if let Some(by) = &by {
                Document::track_format(before, c, by);
            }
        };
        let paras = &self.story(story).paras;
        let mut new = Vec::with_capacity(end.para - start.para + 1);
        for (i, p) in paras.iter().enumerate().take(end.para + 1).skip(start.para) {
            let mut p = p.clone();
            let s = if i == start.para { start.offset } else { 0 };
            let e = if i == end.para { end.offset } else { p.len() };
            p.format_range(s, e, &mut f);
            if i < end.para {
                f(&mut p.mark_chp);
            }
            new.push(p);
        }
        // Formatting that changes nothing is not an edit (and not an undo
        // step).
        if new[..] != self.story(story).paras[start.para..=end.para] {
            self.splice(story, start.para, new.len(), new);
        }
    }

    /// Make `start..end` hyperlink `link` (a number from `crate::link`),
    /// underlined and in `color`. Link 0 removes links instead, giving
    /// the text its paragraph mark's color and underline back. Paragraph
    /// marks are never part of a link.
    pub fn set_link(
        &mut self,
        story: StoryId,
        start: Position,
        end: Position,
        link: u32,
        color: crate::props::Rgb,
    ) {
        let (start, end) = (start.min(end), start.max(end));
        if start == end {
            return;
        }
        let paras = &self.story(story).paras;
        let mut new = Vec::with_capacity(end.para - start.para + 1);
        for (i, p) in paras.iter().enumerate().take(end.para + 1).skip(start.para) {
            let mut p = p.clone();
            let s = if i == start.para { start.offset } else { 0 };
            let e = if i == end.para { end.offset } else { p.len() };
            let mark = p.mark_chp;
            p.format_range(s, e, &mut |c| {
                c.link = link;
                if link == 0 {
                    c.color = mark.color;
                    c.underline = mark.underline;
                } else {
                    c.color = color;
                    c.underline = crate::props::Underline::Single;
                }
            });
            new.push(p);
        }
        if new[..] != self.story(story).paras[start.para..=end.para] {
            self.splice(story, start.para, new.len(), new);
        }
    }

    /// Replace paragraphs `first..=last` with `new`, as Outline view's
    /// Move Up and Move Down do.
    pub fn replace_paragraphs(
        &mut self,
        story: StoryId,
        first: usize,
        last: usize,
        new: Vec<Paragraph>,
    ) {
        self.splice(story, first, last - first + 1, new);
    }

    /// Apply `f` to the paragraph format of paragraphs `first..=last`.
    pub fn format_paras(
        &mut self,
        story: StoryId,
        first: usize,
        last: usize,
        mut f: impl FnMut(&mut ParaProps),
    ) {
        let paras = &self.story(story).paras;
        let new: Vec<Paragraph> = paras[first..=last]
            .iter()
            .map(|p| {
                let mut p = p.clone();
                f(&mut p.pap);
                p
            })
            .collect();
        if new[..] != self.story(story).paras[first..=last] {
            self.splice(story, first, new.len(), new);
        }
    }

    /// Give paragraphs `first..=last` a style the original's way: the paragraph
    /// format becomes the style's, and character formatting that matched
    /// the old style's follows the new style. Differences the user applied
    /// by hand (bold on one word, say) stay.
    pub fn apply_style(&mut self, story: StoryId, first: usize, last: usize, style: u16) {
        let new_style = self.styles.get(style).clone();
        let paras = &self.story(story).paras;
        let new: Vec<Paragraph> = paras[first..=last]
            .iter()
            .map(|p| {
                let old_style = self.styles.get(p.pap.style);
                let mut p = p.clone();
                p.pap = new_style.pap.clone();
                let mut restyle =
                    |c: &mut CharProps| *c = restyle_chp(*c, &old_style.chp, &new_style.chp);
                let len = p.len();
                p.format_range(0, len, &mut restyle);
                restyle(&mut p.mark_chp);
                p
            })
            .collect();
        if new[..] != self.story(story).paras[first..=last] {
            self.splice(story, first, new.len(), new);
        }
    }

    /// Revert To Style: drop hand formatting from paragraphs `first..=last`
    /// so they look exactly like their style.
    pub fn revert_to_style(&mut self, story: StoryId, first: usize, last: usize) {
        let paras = &self.story(story).paras;
        let new: Vec<Paragraph> = paras[first..=last]
            .iter()
            .map(|p| {
                let style = self.styles.get(p.pap.style);
                let mut p = p.clone();
                p.pap = style.pap.clone();
                let base = style.chp;
                let len = p.len();
                p.format_range(0, len, &mut |c| {
                    let object = c.object;
                    *c = base;
                    c.object = object;
                });
                p.mark_chp = base;
                p
            })
            .collect();
        if new[..] != self.story(story).paras[first..=last] {
            self.splice(story, first, new.len(), new);
        }
    }

    // ----- sections -----

    /// The main story's sections, as the paragraphs each one covers: each
    /// ends with a section break, the last with the document.
    pub fn sections(&self) -> Vec<std::ops::RangeInclusive<usize>> {
        let paras = &self.main.paras;
        let mut out = Vec::new();
        let mut first = 0;
        for (i, p) in paras.iter().enumerate() {
            if p.mark == Mark::SectionBreak && i + 1 < paras.len() {
                out.push(first..=i);
                first = i + 1;
            }
        }
        out.push(first..=paras.len() - 1);
        out
    }

    /// The section (from 0) paragraph `para` of the main story is in.
    pub fn section_at(&self, para: usize) -> usize {
        self.main.paras[..para.min(self.main.paras.len())]
            .iter()
            .filter(|p| p.mark == Mark::SectionBreak)
            .count()
    }

    /// The settings of the section that paragraph `para` is in.
    pub fn section_props_at(&self, para: usize) -> &SectionProps {
        let paras = &self.main.paras;
        paras[para.min(paras.len() - 1)..paras.len() - 1]
            .iter()
            .find(|p| p.mark == Mark::SectionBreak)
            .and_then(|p| p.section.as_deref())
            .unwrap_or(&self.section)
    }

    /// Give the sections from the one holding paragraph `first` to the one
    /// holding `last` the settings `f` makes, as one undoable change.
    pub fn format_sections(
        &mut self,
        first: usize,
        last: usize,
        mut f: impl FnMut(&mut SectionProps),
    ) {
        let ranges = self.sections();
        let (a, b) = (self.section_at(first), self.section_at(last));
        for k in a..=b {
            let end = *ranges[k].end();
            if k + 1 == ranges.len() {
                let mut s = self.section.clone();
                f(&mut s);
                if s != self.section {
                    self.record_section();
                    self.section = s;
                }
                continue;
            }
            let mut p = self.main.paras[end].clone();
            let mut s = p
                .section
                .as_deref()
                .cloned()
                .unwrap_or_else(|| self.section.clone());
            f(&mut s);
            if p.section.as_deref() != Some(&s) {
                p.section = Some(Box::new(s));
                self.splice(StoryId::Main, end, 1, vec![p]);
            }
        }
    }

    /// Save the last section's settings in the open undo step, before
    /// changing them.
    fn record_section(&mut self) {
        if self.history.open.is_none() {
            self.begin("Section", Selection::default());
        }
        let sec = Box::new(self.section.clone());
        let step = self.history.open.as_mut().unwrap();
        if step.records.iter().any(|r| r.section.is_some()) {
            return;
        }
        step.records.push(Record {
            story: StoryId::Main,
            first: 0,
            old: Vec::new(),
            new_len: 0,
            sheet: None,
            section: Some(sec),
            props: None,
            summary: None,
            hf_section: 0,
        });
    }

    /// Replace document-wide settings as one undoable action.
    pub fn set_document_props(
        &mut self,
        label: &'static str,
        props: DocumentProps,
        sel: Selection,
    ) -> bool {
        if self.props == props {
            return false;
        }
        self.commit_open();
        self.begin(label, sel);
        let old = std::mem::replace(&mut self.props, props);
        let (was, now) = (old.text_width(), self.props.text_width());
        self.history.open.as_mut().unwrap().records.push(Record {
            story: StoryId::Main,
            first: 0,
            old: Vec::new(),
            new_len: 0,
            sheet: None,
            section: None,
            props: Some(Box::new(old)),
            summary: None,
            hf_section: 0,
        });
        self.fit_full_width_tables(was, now);
        self.commit(sel);
        true
    }

    /// Change the document settings as part of the step already open (see
    /// `begin`), so they undo with the rest of it.
    pub fn change_document_props(&mut self, props: DocumentProps) {
        if self.props == props {
            return;
        }
        let old = std::mem::replace(&mut self.props, props);
        if let Some(step) = self.history.open.as_mut() {
            step.records.push(Record {
                story: StoryId::Main,
                first: 0,
                old: Vec::new(),
                new_len: 0,
                sheet: None,
                section: None,
                props: Some(Box::new(old)),
                summary: None,
                hf_section: 0,
            });
        }
    }

    /// Replace File > Summary Info as one undoable action.
    pub fn set_summary(&mut self, summary: SummaryInfo, sel: Selection) -> bool {
        if self.summary == summary {
            return false;
        }
        self.commit_open();
        self.begin("Summary Info", sel);
        let old = std::mem::replace(&mut self.summary, summary);
        self.history.open.as_mut().unwrap().records.push(Record {
            story: StoryId::Main,
            first: 0,
            old: Vec::new(),
            new_len: 0,
            sheet: None,
            section: None,
            props: None,
            summary: Some(Box::new(old)),
            hf_section: 0,
        });
        self.commit(sel);
        true
    }

    /// Replace the final section's settings as one undoable action.
    pub fn set_last_section(
        &mut self,
        label: &'static str,
        section: SectionProps,
        sel: Selection,
    ) -> bool {
        if self.section == section {
            return false;
        }
        self.commit_open();
        self.begin(label, sel);
        self.record_section();
        self.section = section;
        self.commit(sel);
        true
    }

    /// ⌘Enter: end the section at `at` with a section break. The section
    /// before it keeps the settings the whole section had. Returns the
    /// start of the new section.
    pub fn insert_section_break(&mut self, at: Position) -> Position {
        let props = self.section_props_at(at.para).clone();
        let mut head = self.main.paras[at.para].clone();
        let tail = head.split_off(at.offset);
        head.mark = Mark::SectionBreak;
        head.section = Some(Box::new(props));
        self.splice(StoryId::Main, at.para, 1, vec![head, tail]);
        // The new section after the break shows the one before's headers
        // and footers (Same As Previous), as in the original.
        self.format_sections(at.para + 1, at.para + 1, |s| s.headers = None);
        Position::new(at.para + 1, 0)
    }

    // ----- the style sheet -----

    /// Save the style sheet in the open undo step, before changing it.
    fn record_sheet(&mut self) {
        if self.history.open.is_none() {
            self.begin("Style", Selection::default());
        }
        let sheet = Box::new(self.styles.clone());
        let step = self.history.open.as_mut().unwrap();
        // One copy per step is enough: undo restores the earliest.
        if step.records.iter().any(|r| r.sheet.is_some()) {
            return;
        }
        step.records.push(Record {
            story: StoryId::Main,
            first: 0,
            old: Vec::new(),
            new_len: 0,
            sheet: Some(sheet),
            section: None,
            props: None,
            summary: None,
            hf_section: 0,
        });
    }

    /// Define a new style, as the Style dialog's Define does. `next` is the
    /// style for the paragraph after; None means the style itself.
    pub fn add_style(
        &mut self,
        name: &str,
        based_on: StyleId,
        next: Option<StyleId>,
        pap: ParaProps,
        chp: CharProps,
    ) -> StyleId {
        self.record_sheet();
        let id = self.styles.add(name, based_on, pap, chp);
        self.styles.styles[id as usize].next = next.unwrap_or(id);
        id
    }

    /// Change a style's name, Based On and Next Style.
    pub fn rename_style(
        &mut self,
        id: StyleId,
        name: &str,
        based_on: Option<StyleId>,
        next: StyleId,
    ) {
        let st = self.styles.get(id);
        if st.name == name && st.based_on == based_on && st.next == next {
            return;
        }
        self.record_sheet();
        let st = &mut self.styles.styles[id as usize];
        // Automatic styles keep their names.
        if !st.builtin {
            st.name = name.to_string();
        }
        // Normal is based on nothing, and nothing is based on itself.
        st.based_on = if id == 0 {
            None
        } else {
            based_on.filter(|&b| b != id)
        };
        st.next = next;
    }

    /// Give a style new formatting. Paragraphs in the style, and styles
    /// based on it, follow, keeping whatever they set differently.
    pub fn redefine_style(&mut self, story: StoryId, id: StyleId, pap: ParaProps, chp: CharProps) {
        self.record_sheet();
        self.redefine(story, id, pap, chp, 0);
    }

    fn redefine(&mut self, story: StoryId, id: StyleId, pap: ParaProps, chp: CharProps, depth: u8) {
        let old = self.styles.get(id).clone();
        let mut pap = pap;
        pap.style = id;
        let st = &mut self.styles.styles[id as usize];
        st.pap = pap.clone();
        st.chp = chp;
        // Restyle the paragraphs.
        for i in 0..self.story(story).paras.len() {
            let p = &self.story(story).paras[i];
            if p.pap.style != id {
                continue;
            }
            let mut p = p.clone();
            p.pap = restyle_pap(&p.pap, &old.pap, &pap);
            let len = p.len();
            p.format_range(0, len, &mut |c| *c = restyle_chp(*c, &old.chp, &chp));
            p.mark_chp = restyle_chp(p.mark_chp, &old.chp, &chp);
            if p != self.story(story).paras[i] {
                self.splice(story, i, 1, vec![p]);
            }
        }
        // Styles based on this one inherit the change.
        if depth > 8 {
            return;
        }
        let derived: Vec<StyleId> = (0..self.styles.styles.len() as StyleId)
            .filter(|&k| k != id && self.styles.get(k).based_on == Some(id))
            .collect();
        for k in derived {
            let s = self.styles.get(k).clone();
            let dp = restyle_pap(&s.pap, &old.pap, &pap);
            let dc = restyle_chp(s.chp, &old.chp, &chp);
            self.redefine(story, k, dp, dc, depth + 1);
        }
    }

    /// Delete a style the user defined. Its paragraphs go back to Normal.
    pub fn delete_style(&mut self, story: StoryId, id: StyleId) {
        if id == 0 || self.styles.get(id).builtin || id as usize >= self.styles.styles.len() {
            return;
        }
        let n = self.story(story).paras.len();
        let uses = self.story(story).paras.iter().any(|p| p.pap.style == id);
        if uses {
            // apply_style one paragraph at a time keeps hand formatting.
            for i in 0..n {
                if self.story(story).paras[i].pap.style == id {
                    self.apply_style(story, i, i, 0);
                }
            }
        }
        self.record_sheet();
        let gone = self.styles.styles.remove(id as usize);
        let fix = |s: StyleId| if s > id { s - 1 } else { s };
        for (k, st) in self.styles.styles.iter_mut().enumerate() {
            st.based_on = st
                .based_on
                .map(|b| {
                    if b == id {
                        gone.based_on.unwrap_or(0)
                    } else {
                        b
                    }
                })
                .map(fix);
            st.next = if st.next == id {
                k as StyleId
            } else {
                fix(st.next)
            };
            st.pap.style = k as StyleId;
        }
        for i in 0..n {
            let s = self.story(story).paras[i].pap.style;
            if s > id {
                let mut p = self.story(story).paras[i].clone();
                p.pap.style = s - 1;
                self.splice(story, i, 1, vec![p]);
            }
        }
    }
}

/// Move a paragraph format from one style to another, keeping anything
/// that differs from the old style.
fn restyle_pap(p: &ParaProps, old: &ParaProps, new: &ParaProps) -> ParaProps {
    macro_rules! carry {
        ($out:ident, $($field:ident),*) => {
            $( if p.$field == old.$field { $out.$field = new.$field.clone(); } )*
        };
    }
    let mut out = p.clone();
    carry!(
        out,
        align,
        left,
        right,
        first,
        before,
        after,
        line,
        tabs,
        page_break_before,
        keep_with_next,
        keep_together,
        suppress_line_numbers,
        borders,
        shading
    );
    out
}

/// Move a character format from one style to another, keeping anything
/// that differs from the old style (hand formatting).
fn restyle_chp(c: CharProps, old: &CharProps, new: &CharProps) -> CharProps {
    macro_rules! carry {
        ($out:ident, $($field:ident),*) => {
            $( if c.$field == old.$field { $out.$field = new.$field; } )*
        };
    }
    let mut out = c;
    carry!(
        out, font, size, bold, italic, underline, strike, outline, shadow, small_caps, all_caps,
        hidden, color, position, spacing
    );
    out
}

fn intern(fonts: &mut Vec<String>, name: &str) -> FontId {
    if let Some(i) = fonts.iter().position(|f| f.eq_ignore_ascii_case(name)) {
        return i as FontId;
    }
    fonts.push(name.to_string());
    (fonts.len() - 1) as FontId
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::Align;

    const MAIN: StoryId = StoryId::Main;

    fn p(para: usize, offset: usize) -> Position {
        Position::new(para, offset)
    }

    fn texts(doc: &Document) -> Vec<&str> {
        doc.main.paras.iter().map(|p| p.text.as_str()).collect()
    }

    fn type_text(doc: &mut Document, at: Position, text: &str) -> Position {
        doc.begin_typing(Selection::caret(at));
        let chp = doc.main.paras[at.para].chp_at(at.offset);
        let end = doc.insert_plain(MAIN, at, text, chp);
        doc.commit(Selection::caret(end));
        end
    }

    fn doc_with(paras: &[&str]) -> Document {
        let mut d = Document::new();
        type_text(&mut d, p(0, 0), &paras.join("\n"));
        d.history = History::default();
        d.saved_state = 0;
        d
    }

    #[test]
    fn new_document_is_new_york_12() {
        let d = Document::new();
        assert_eq!(d.main.paras.len(), 1);
        let chp = d.main.paras[0].mark_chp;
        assert_eq!(d.font_name(chp.font), "New York");
        assert_eq!(chp.size, 24);
        assert!(!d.is_modified());
    }

    #[test]
    fn typing_paragraphs() {
        let d = doc_with(&["Hello", "World"]);
        assert_eq!(texts(&d), ["Hello", "World"]);
    }

    #[test]
    fn trailing_newline_leaves_empty_paragraph() {
        let mut d = Document::new();
        let end = type_text(&mut d, p(0, 0), "One\n");
        assert_eq!(texts(&d), ["One", ""]);
        assert_eq!(end, p(1, 0));
    }

    #[test]
    fn return_splits_and_keeps_format() {
        let mut d = doc_with(&["Hello world"]);
        d.format_paras(MAIN, 0, 0, |pp| pp.align = Align::Center);
        let at = d.split_paragraph(MAIN, p(0, 5));
        assert_eq!(at, p(1, 0));
        assert_eq!(texts(&d), ["Hello", " world"]);
        assert_eq!(d.main.paras[1].pap.align, Align::Center);
    }

    #[test]
    fn return_after_heading_gives_normal() {
        let mut d = doc_with(&["Title"]);
        let h1 = d.styles.find("heading 1").unwrap();
        d.apply_style(MAIN, 0, 0, h1);
        d.split_paragraph(MAIN, p(0, 5));
        assert_eq!(d.main.paras[0].pap.style, h1);
        assert_eq!(d.main.paras[1].pap.style, 0);
        assert!(!d.main.paras[1].mark_chp.bold);
    }

    #[test]
    fn deleting_mark_takes_following_format() {
        let mut d = doc_with(&["First", "Second"]);
        d.format_paras(MAIN, 1, 1, |pp| pp.align = Align::Right);
        d.delete(MAIN, p(0, 5), p(1, 0));
        assert_eq!(texts(&d), ["FirstSecond"]);
        assert_eq!(d.main.paras[0].pap.align, Align::Right);
    }

    #[test]
    fn delete_across_paragraphs() {
        let mut d = doc_with(&["Hello there", "big", "wide world"]);
        let at = d.delete(MAIN, p(0, 5), p(2, 4));
        assert_eq!(at, p(0, 5));
        assert_eq!(texts(&d), ["Hello world"]);
    }

    #[test]
    fn delete_whole_paragraph_with_mark() {
        let mut d = doc_with(&["One", "Two", "Three"]);
        d.delete(MAIN, p(1, 0), p(2, 0));
        assert_eq!(texts(&d), ["One", "Three"]);
    }

    #[test]
    fn undo_and_redo() {
        let mut d = doc_with(&["Hello"]);
        d.begin("Clear", Selection::new(p(0, 0), p(0, 5)));
        d.delete(MAIN, p(0, 0), p(0, 5));
        d.commit(Selection::caret(p(0, 0)));
        assert_eq!(texts(&d), [""]);
        assert_eq!(d.undo_label(), Some("Clear"));
        let sel = d.undo().unwrap();
        assert_eq!(sel, Selection::new(p(0, 0), p(0, 5)));
        assert_eq!(texts(&d), ["Hello"]);
        assert_eq!(d.redo_label(), Some("Clear"));
        let sel = d.redo().unwrap();
        assert_eq!(sel, Selection::caret(p(0, 0)));
        assert_eq!(texts(&d), [""]);
    }

    #[test]
    fn undo_keeps_at_most_one_hundred_recent_actions() {
        let mut d = Document::new();
        let mut at = p(0, 0);
        for _ in 0..120 {
            d.begin("Insert", Selection::caret(at));
            let chp = d.main.paras[0].chp_at(at.offset);
            at = d.insert_plain(MAIN, at, "x", chp);
            d.commit(Selection::caret(at));
        }

        let mut undone = 0;
        while d.undo().is_some() {
            undone += 1;
        }

        assert_eq!(undone, 100);
        assert_eq!(d.main.paras[0].text.len(), 20);
        assert!(d.is_modified());
    }

    #[test]
    fn undo_memory_budget_never_discards_the_newest_twenty_five_actions() {
        let mut d = Document::new();
        let paragraph = || {
            Paragraph::with_text(
                &"x".repeat(1024),
                CharProps::default(),
                ParaProps::default(),
            )
        };
        d.history.undo = (1..=30)
            .map(|id| Step {
                label: "Large edit",
                records: vec![Record {
                    story: MAIN,
                    first: 0,
                    old: vec![paragraph()],
                    new_len: 1,
                    sheet: None,
                    section: None,
                    props: None,
                    summary: None,
                    hf_section: 0,
                }],
                before: Selection::default(),
                after: Selection::default(),
                id,
            })
            .collect();

        d.trim_history(25, 100, 0, false);

        assert_eq!(d.history.undo.len(), 25);
        assert_eq!(d.history.base_id, 5);
        assert!(d.undo_owned_bytes() > 0);
    }

    #[test]
    fn one_action_larger_than_the_budget_is_still_undoable() {
        let mut d = Document::new();
        let old = Paragraph::with_text(
            &"x".repeat(1024 * 1024),
            CharProps::default(),
            ParaProps::default(),
        );
        d.history.undo.push(Step {
            label: "Large edit",
            records: vec![Record {
                story: MAIN,
                first: 0,
                old: vec![old],
                new_len: 1,
                sheet: None,
                section: None,
                props: None,
                summary: None,
                hf_section: 0,
            }],
            before: Selection::default(),
            after: Selection::default(),
            id: 1,
        });

        d.trim_history(25, 100, 0, false);

        assert_eq!(d.history.undo.len(), 1);
        assert_eq!(d.undo_label(), Some("Large edit"));
    }

    #[test]
    fn pruning_history_releases_its_unplaced_picture_data() {
        use crate::picture::{Crop, ImageFormat};
        use std::sync::Arc;

        let mut d = Document::new();
        let data: Arc<[u8]> = vec![7; 4096].into();
        let preview: Arc<[u8]> = vec![8; 1024].into();
        let weak_data = Arc::downgrade(&data);
        let weak_preview = Arc::downgrade(&preview);
        let picture = Picture {
            data: data.clone(),
            preview: Some(preview.clone()),
            preview_pixels: Some((1, 1)),
            format: ImageFormat::Png,
            pixels: (1, 1),
            natural: (20, 20),
            size: (20, 20),
            crop: Crop::default(),
        };
        drop(data);
        drop(preview);
        let chp = d.main.paras[0].mark_chp;
        let end = d.insert_picture(MAIN, p(0, 0), picture, chp);
        d.commit(Selection::caret(end));
        d.begin("Delete picture", Selection::caret(p(0, 0)));
        d.delete(MAIN, p(0, 0), end);
        d.commit(Selection::caret(p(0, 0)));
        d.reclaim_unreachable_objects();
        assert!(weak_data.upgrade().is_some());
        assert!(weak_preview.upgrade().is_some());

        let mut at = p(0, 0);
        for _ in 0..100 {
            d.begin("Insert", Selection::caret(at));
            at = d.insert_plain(MAIN, at, "x", chp);
            d.commit(Selection::caret(at));
        }

        assert!(weak_data.upgrade().is_none());
        assert!(weak_preview.upgrade().is_none());
        assert_eq!(d.object(1), None);
        assert_eq!(d.objects[0], Object::Vacant);
    }

    #[test]
    fn typing_coalesces_into_one_undo() {
        let mut d = Document::new();
        let mut at = p(0, 0);
        for c in ["a", "b", "c"] {
            at = type_text(&mut d, at, c);
        }
        assert_eq!(texts(&d), ["abc"]);
        d.undo();
        assert_eq!(texts(&d), [""]);
        assert!(d.undo_label().is_none());
    }

    #[test]
    fn moving_the_caret_breaks_typing() {
        let mut d = Document::new();
        let at = type_text(&mut d, p(0, 0), "ab");
        d.break_typing();
        type_text(&mut d, at, "c");
        d.undo();
        assert_eq!(texts(&d), ["ab"]);
    }

    #[test]
    fn multi_paragraph_undo_restores_everything() {
        let mut d = doc_with(&["One", "Two", "Three"]);
        let before = d.main.clone();
        d.begin("Cut", Selection::default());
        d.delete(MAIN, p(0, 1), p(2, 2));
        d.insert_plain(MAIN, p(0, 1), "x\ny\nz", CharProps::default());
        d.commit(Selection::default());
        d.undo();
        assert_eq!(d.main, before);
        d.redo();
        assert_eq!(texts(&d), ["Ox", "y", "zree"]);
    }

    #[test]
    fn modified_tracks_saves_and_undo() {
        let mut d = Document::new();
        assert!(!d.is_modified());
        type_text(&mut d, p(0, 0), "a");
        assert!(d.is_modified());
        d.mark_saved();
        assert!(!d.is_modified());
        d.undo();
        assert!(d.is_modified());
        d.redo();
        assert!(!d.is_modified());
    }

    #[test]
    fn document_settings_and_summary_are_undoable() {
        let mut d = Document::new();
        let at = type_text(&mut d, p(0, 0), "x");
        let sel = Selection::caret(at);
        let original_props = d.props.clone();
        let mut props = original_props.clone();
        props.margin_left = 2160;
        assert!(d.set_document_props("Document", props.clone(), sel));
        assert_eq!(d.undo_label(), Some("Document"));

        let summary = SummaryInfo {
            title: "A title".into(),
            ..SummaryInfo::default()
        };
        assert!(d.set_summary(summary.clone(), sel));
        assert_eq!(d.undo_label(), Some("Summary Info"));
        assert!(d.is_modified());

        assert_eq!(d.undo(), Some(sel));
        assert_eq!(d.summary, SummaryInfo::default());
        assert_eq!(d.props, props);
        assert_eq!(d.undo(), Some(sel));
        assert_eq!(d.props, original_props);
        assert_eq!(d.main.paras[0].text, "x", "typing is a separate step");
        assert_eq!(d.redo(), Some(sel));
        assert_eq!(d.props, props);
        assert_eq!(d.redo(), Some(sel));
        assert_eq!(d.summary, summary);
    }

    #[test]
    fn copy_and_paste_within_paragraph() {
        let mut d = doc_with(&["Hello world"]);
        d.format_chars(MAIN, p(0, 6), p(0, 11), |c| c.bold = true);
        let frag = d.copy(MAIN, p(0, 6), p(0, 11));
        assert!(!frag.ends_with_mark);
        let end = d.insert_fragment(MAIN, p(0, 0), &frag);
        assert_eq!(end, p(0, 5));
        assert_eq!(texts(&d), ["worldHello world"]);
        assert!(d.main.paras[0].chp_of_char(0).bold);
        assert!(!d.main.paras[0].chp_of_char(5).bold);
    }

    #[test]
    fn copy_and_paste_paragraphs() {
        let mut d = doc_with(&["Alpha", "Beta", "Gamma"]);
        d.format_paras(MAIN, 1, 1, |pp| pp.align = Align::Center);
        let frag = d.copy(MAIN, p(0, 2), p(1, 2));
        assert_eq!(frag.plain_text(), "pha\nBe");
        let end = d.insert_fragment(MAIN, p(2, 1), &frag);
        assert_eq!(texts(&d), ["Alpha", "Beta", "Gpha", "Beamma"]);
        assert_eq!(end, p(3, 2));
        // The "Be" piece joined Gamma's remainder and keeps Gamma's format.
        assert_eq!(d.main.paras[3].pap.align, Align::Left);
    }

    #[test]
    fn paste_whole_paragraphs_with_mark() {
        let mut d = doc_with(&["One", "Two"]);
        let frag = d.copy(MAIN, p(0, 0), p(1, 0));
        assert!(frag.ends_with_mark);
        let end = d.insert_fragment(MAIN, p(1, 0), &frag);
        assert_eq!(texts(&d), ["One", "One", "Two"]);
        assert_eq!(end, p(2, 0));
    }

    #[test]
    fn set_link_and_remove_it() {
        let mut d = doc_with(&["One two", "three"]);
        let id = crate::link::intern(crate::link::Link::new("https://example.com/"));
        d.set_link(MAIN, p(0, 4), p(1, 2), id, crate::props::Rgb(0x0000EE));
        assert_eq!(d.main.paras[0].chp_of_char(4).link, id);
        assert_eq!(d.main.paras[1].chp_of_char(1).link, id);
        assert_eq!(d.main.paras[1].chp_of_char(2).link, 0);
        assert_eq!(d.main.paras[0].mark_chp.link, 0, "marks are never linked");
        assert_eq!(
            d.main.paras[0].chp_of_char(5).underline,
            crate::props::Underline::Single
        );
        d.set_link(MAIN, p(0, 0), p(1, 5), 0, crate::props::Rgb::BLACK);
        let back = d.main.paras[0].chp_of_char(5);
        assert_eq!(back.link, 0);
        assert_eq!(back.underline, d.main.paras[0].mark_chp.underline);
        assert_eq!(back.color, d.main.paras[0].mark_chp.color);
    }

    #[test]
    fn format_chars_includes_crossed_marks() {
        let mut d = doc_with(&["One", "Two"]);
        d.format_chars(MAIN, p(0, 1), p(1, 1), |c| c.italic = true);
        assert!(d.main.paras[0].mark_chp.italic);
        assert!(!d.main.paras[1].mark_chp.italic);
        assert!(d.main.paras[1].chp_of_char(0).italic);
        assert!(!d.main.paras[1].chp_of_char(1).italic);
    }

    #[test]
    fn apply_style_keeps_hand_formatting() {
        let mut d = doc_with(&["Some words"]);
        d.format_chars(MAIN, p(0, 0), p(0, 4), |c| c.italic = true);
        let h1 = d.styles.find("heading 1").unwrap();
        d.apply_style(MAIN, 0, 0, h1);
        let para = &d.main.paras[0];
        assert!(para.chp_of_char(0).bold, "style's bold applied");
        assert!(para.chp_of_char(0).italic, "hand italic kept");
        assert!(!para.chp_of_char(5).italic);
        assert_eq!(d.font_name(para.chp_of_char(5).font), "Helvetica");
        d.revert_to_style(MAIN, 0, 0);
        assert!(!d.main.paras[0].chp_of_char(0).italic);
    }

    #[test]
    fn formatting_that_changes_nothing_is_not_an_undo_step() {
        let mut d = doc_with(&["Plain"]);
        d.begin("Formatting", Selection::default());
        d.format_chars(MAIN, p(0, 0), p(0, 5), |c| c.bold = false);
        d.format_paras(MAIN, 0, 0, |pp| pp.align = Align::Left);
        d.commit(Selection::default());
        assert!(d.undo_label().is_none());
        assert!(!d.is_modified());
    }

    #[test]
    fn unchanged_style_actions_preserve_saved_point_and_redo() {
        let mut d = doc_with(&["Plain"]);
        let sel = Selection::default();
        d.take_changes();
        // Repetition must not fill history or evict useful earlier actions.
        for _ in 0..150 {
            d.begin("Style", sel);
            d.apply_style(MAIN, 0, 0, 0);
            d.commit(sel);
            d.begin("Revert To Style", sel);
            d.revert_to_style(MAIN, 0, 0);
            d.commit(sel);
        }
        assert!(!d.is_modified());
        assert_eq!(d.undo_label(), None);
        assert!(d.take_changes().is_empty());
        d.begin("Bold", sel);
        d.format_chars(MAIN, p(0, 0), p(0, 5), |c| c.bold = true);
        d.commit(sel);
        d.undo();
        let redo = d.redo_label();
        assert!(redo.is_some());
        d.begin("Style", sel);
        d.apply_style(MAIN, 0, 0, 0);
        d.revert_to_style(MAIN, 0, 0);
        d.commit(sel);
        assert!(!d.is_modified());
        assert_eq!(d.redo_label(), redo);
        d.redo();
        assert!(d.main.paras[0].chp_at(0).bold);
        d.mark_saved();
        d.begin("Revert To Style", sel);
        d.revert_to_style(MAIN, 0, 0);
        d.commit(sel);
        assert!(!d.main.paras[0].chp_at(0).bold);
        assert!(d.is_modified());
        d.undo();
        assert!(d.main.paras[0].chp_at(0).bold);
        assert!(!d.is_modified());
    }

    #[test]
    fn changes_are_logged() {
        let mut d = doc_with(&["a"]);
        d.take_changes();
        d.split_paragraph(MAIN, p(0, 1));
        assert_eq!(
            d.take_changes(),
            [Change {
                story: MAIN,
                first: 0,
                removed: 1,
                inserted: 2
            }]
        );
        d.undo();
        assert_eq!(
            d.take_changes(),
            [Change {
                story: MAIN,
                first: 0,
                removed: 2,
                inserted: 1
            }]
        );
    }

    #[test]
    fn defining_a_style_can_be_undone() {
        let mut d = doc_with(&["One", "Two"]);
        let n = d.styles.styles.len();
        let pap = d.main.paras[0].pap.clone();
        let chp = d.main.paras[0].chp_at(0);
        d.begin("Define Style", Selection::default());
        let id = d.add_style("Quote", 0, None, pap, chp);
        d.commit(Selection::default());
        assert_eq!(d.styles.get(id).name, "Quote");
        assert_eq!(d.styles.get(id).next, id);
        d.undo();
        assert_eq!(d.styles.styles.len(), n);
        d.redo();
        assert_eq!(d.styles.find("Quote"), Some(id));
    }

    #[test]
    fn redefining_follows_paragraphs_and_based_on_styles() {
        let mut d = doc_with(&["Plain", "Head"]);
        let h1 = d.styles.find("heading 1").unwrap();
        d.apply_style(MAIN, 1, 1, h1);
        // Hand formatting: italic on "Pl".
        d.format_chars(MAIN, p(0, 0), p(0, 2), |c| c.italic = true);
        d.commit(Selection::default());
        let mut pap = d.styles.get(0).pap.clone();
        pap.left = 720;
        let mut chp = d.styles.get(0).chp;
        chp.italic = false;
        chp.size = 20;
        d.begin("Style", Selection::default());
        d.redefine_style(MAIN, 0, pap, chp);
        d.commit(Selection::default());
        let para = &d.main.paras[0];
        assert_eq!(para.pap.left, 720);
        assert_eq!(para.chp_at(3).size, 20);
        assert!(para.chp_at(1).italic, "hand formatting stays");
        // heading 1 sets its own font, so it keeps it, but takes the
        // indent and the size it shared with Normal.
        let h = d.styles.get(h1);
        assert_eq!(h.pap.left, 720);
        assert_eq!(h.chp.size, 20);
        assert_eq!(d.font_name(h.chp.font), "Helvetica");
        assert_eq!(d.main.paras[1].pap.left, 720);
        d.undo();
        assert_eq!(d.main.paras[0].pap.left, 0);
        assert_eq!(d.styles.get(0).pap.left, 0);
    }

    #[test]
    fn deleting_a_style_returns_its_paragraphs_to_normal() {
        let mut d = doc_with(&["A", "B", "C"]);
        let pap = d.main.paras[0].pap.clone();
        let mut chp = d.main.paras[0].chp_at(0);
        chp.bold = true;
        d.begin("Style", Selection::default());
        let a = d.add_style("Alpha", 0, None, pap.clone(), chp);
        let b = d.add_style("Beta", a, None, pap, chp);
        d.apply_style(MAIN, 0, 0, a);
        d.apply_style(MAIN, 2, 2, b);
        d.commit(Selection::default());
        d.begin("Delete Style", Selection::default());
        d.delete_style(MAIN, a);
        d.commit(Selection::default());
        assert_eq!(d.main.paras[0].pap.style, 0);
        assert!(!d.main.paras[0].chp_at(0).bold);
        let beta = d.styles.find("Beta").unwrap();
        assert_eq!(beta, b - 1);
        assert_eq!(d.main.paras[2].pap.style, beta);
        assert_eq!(d.styles.get(beta).based_on, Some(0));
        assert_eq!(d.styles.get(beta).pap.style, beta);
        d.undo();
        assert_eq!(d.styles.find("Alpha"), Some(a));
        assert_eq!(d.main.paras[0].pap.style, a);
        assert_eq!(d.main.paras[2].pap.style, b);
    }

    #[test]
    fn importing_a_story_maps_fonts_by_name() {
        let mut d = doc_with(&["Body"]);
        let mut other = d.clone();
        let geneva = other.font_id("Geneva");
        let mut header = other.story(StoryId::Header).clone();
        header.paras[0].text = "Head".into();
        header.paras[0].runs = vec![crate::para::Run {
            len: 4,
            chp: CharProps {
                font: geneva,
                ..d.main.paras[0].chp_at(0)
            },
        }];
        // The real document meanwhile gained a different font.
        d.font_id("Monaco");
        d.import_story(StoryId::Header, &header, &other.fonts, &other.objects);
        let p = &d.story(StoryId::Header).paras[0];
        assert_eq!(p.text, "Head");
        assert_eq!(d.font_name(p.chp_at(0).font), "Geneva");
        assert!(d.is_modified());
        assert!(!d.story_is_empty(StoryId::Header));
        assert!(d.story_is_empty(StoryId::Footer));
    }

    #[test]
    fn sections_have_their_own_headers_or_share() {
        let mut d = crate::text::from_plain_text("One\nTwo");
        let chp = d.main.paras[0].mark_chp;
        d.insert_text(StoryId::Header, Position::new(0, 0), "First", chp);
        d.forget_history();
        // A new section shares the one before's headers.
        d.begin("Break", Selection::default());
        d.insert_section_break(Position::new(1, 0));
        d.commit(Selection::default());
        assert!(!d.has_own_headers(1));
        assert_eq!(d.header_story(1, StoryId::Header).paras[0].text, "First");
        // Typing into its header gives it its own; the first is untouched.
        d.header_section = 1;
        d.begin("Typing", Selection::default());
        d.insert_text(StoryId::Header, Position::new(0, 0), "Second ", chp);
        d.commit(Selection::default());
        assert!(d.has_own_headers(1));
        assert_eq!(
            d.header_story(1, StoryId::Header).paras[0].text,
            "Second First"
        );
        assert_eq!(d.header_story(0, StoryId::Header).paras[0].text, "First");
        // Undo puts the second section's text back, wherever the pane is.
        d.header_section = 0;
        d.undo();
        assert_eq!(d.header_story(1, StoryId::Header).paras[0].text, "First");
        assert_eq!(d.header_story(0, StoryId::Header).paras[0].text, "First");
        d.redo();
        // Same As Previous shares the first section's again, undoably.
        d.begin("Same As Previous", Selection::default());
        d.link_headers(1);
        d.commit(Selection::default());
        assert!(!d.has_own_headers(1));
        assert_eq!(d.header_story(1, StoryId::Header).paras[0].text, "First");
        d.undo();
        assert_eq!(
            d.header_story(1, StoryId::Header).paras[0].text,
            "Second First"
        );
    }

    #[test]
    fn importing_a_story_keeps_the_ids_on_comment_marks() {
        let mut other = Document::new();
        let mut doc = Document::new();
        let id = other.insert_annotation(
            Position::new(0, 0),
            other.main.paras[0].mark_chp,
            crate::notes::AnnotationInfo::default(),
        );
        // A picture list the id could be mistaken for a place in: the
        // other document's first picture is this one's second.
        let png = |w: u8| {
            let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0".to_vec();
            v.extend_from_slice(&[w, 0, 0, 0, 1, 8, 2, 0, 0, 0]);
            crate::Picture::from_bytes(v).unwrap()
        };
        other.add_object(Object::Picture(png(3)));
        doc.add_object(Object::Picture(png(2)));
        doc.add_object(Object::Picture(png(3)));
        let mark_story = Story {
            paras: vec![other.main.paras[0].clone()],
        };
        doc.import_story(
            StoryId::Annotations,
            &mark_story,
            &other.fonts,
            &other.objects,
        );
        let p = &doc.annotations.paras[0];
        assert!(p.text.starts_with(chars::ANNOTATION));
        assert_eq!(p.chp_of_char(0).object, id);
    }

    #[test]
    fn header_lookups_agree_for_every_section() {
        let mut d = crate::text::from_plain_text("A\nB\nC\nD\nE");
        let chp = d.main.paras[0].mark_chp;
        d.insert_text(StoryId::Header, Position::new(0, 0), "Zero", chp);
        d.forget_history();
        for para in [3, 2, 1] {
            d.begin("Break", Selection::default());
            d.insert_section_break(Position::new(para, 0));
            d.commit(Selection::default());
        }
        // Sections 1 and 3 get headers of their own; 2 shares the one before's.
        for (k, text) in [(1, "One "), (3, "Three ")] {
            d.header_section = k;
            d.begin("Typing", Selection::default());
            d.insert_text(StoryId::Header, Position::new(0, 0), text, chp);
            d.commit(Selection::default());
        }
        let ranges = d.sections();
        assert_eq!(ranges.len(), 4);
        let sets = d.header_sets();
        assert_eq!(sets.len(), ranges.len());
        for k in 0..ranges.len() + 2 {
            // The slow way: walk down from section k.
            let mut want = k.min(ranges.len() - 1);
            while want > 0 && d.nth_section(want).headers.is_none() {
                want -= 1;
            }
            assert_eq!(d.header_source(k), want, "source of {k}");
            if let Some((source, set)) = sets.get(k) {
                assert_eq!(*source, want, "set source of {k}");
                assert_eq!(set[0].paras[0].text, d.headers_of(k)[0].paras[0].text);
            }
        }
        assert_eq!(d.header_story(2, StoryId::Header).paras[0].text, "One Zero");
        assert_eq!(
            d.header_story(3, StoryId::Header).paras[0].text,
            "Three One Zero"
        );
        for (k, r) in ranges.iter().enumerate() {
            let last = k + 1 == ranges.len();
            let want = if last {
                &d.section
            } else {
                d.main.paras[*r.end()]
                    .section
                    .as_deref()
                    .unwrap_or(&d.section)
            };
            assert!(std::ptr::eq(d.nth_section(k), want), "section {k}");
        }
    }

    #[test]
    fn importing_a_story_brings_its_pictures() {
        let mut d = Document::new();
        // The header pane's copy, where a picture goes into the header.
        let mut pane = d.clone();
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x01\x08\x02\0\0\0".to_vec();
        let pic = crate::Picture::from_bytes(png).unwrap();
        let chp = pane.main.paras[0].mark_chp;
        pane.main = pane.story(StoryId::Header).clone();
        pane.insert_picture(StoryId::Main, Position::new(0, 0), pic.clone(), chp);
        // The real document gains a picture of its own meanwhile.
        let other = crate::Picture {
            size: (100, 100),
            ..pic.clone()
        };
        d.insert_picture(StoryId::Main, Position::new(0, 0), other, chp);
        d.import_story(StoryId::Header, &pane.main, &pane.fonts, &pane.objects);
        let id = d.story(StoryId::Header).paras[0].chp_of_char(0).object;
        assert_eq!(d.picture(id), Some(&pic));
        // Importing again does not add it twice.
        let n = d.objects.len();
        d.import_story(StoryId::Header, &pane.main, &pane.fonts, &pane.objects);
        assert_eq!(d.objects.len(), n);
    }

    #[test]
    fn pane_copy_contains_only_the_chosen_story_and_no_history() {
        let mut d = Document::new();
        type_text(&mut d, Position::new(0, 0), "A long body");
        let chp = d.main.paras[0].mark_chp;
        d.story_mut(StoryId::Header).paras[0].insert(0, "Header", chp);

        let pane = d.copy_story_for_edit(StoryId::Header);
        assert_eq!(pane.main.paras[0].text, "Header");
        assert!(!pane.main.paras[0].text.contains("long body"));
        assert_eq!(pane.undo_label(), None);
        assert!(!pane.is_modified());
    }
}
