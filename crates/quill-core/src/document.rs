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

impl SectionProps {
    /// The width of one of this section's columns, in a text area `width`
    /// wide.
    pub fn column_width(&self, width: Twips) -> Twips {
        let n = self.columns.max(1) as Twips;
        ((width - (n - 1) * self.column_spacing) / n).max(144)
    }
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
    /// The view the document was saved in, to open it in again. None for
    /// a new document, or a file that does not say.
    pub view: Option<SavedView>,
    /// How superscripts and subscripts are drawn (Format > Document).
    pub scripts: crate::props::ScriptSizes,
}

/// A view a document can be saved in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedView {
    Normal,
    Outline,
    PageLayout,
}

impl DocumentProps {
    /// Take the paper and margins from `from`: for a file whose format has
    /// no page of its own, which opens on the person's default paper.
    pub fn take_paper(&mut self, from: &DocumentProps) {
        self.page_width = from.page_width;
        self.page_height = from.page_height;
        self.margin_left = from.margin_left;
        self.margin_right = from.margin_right;
        self.margin_top = from.margin_top;
        self.margin_bottom = from.margin_bottom;
        self.top_exact = from.top_exact;
        self.bottom_exact = from.bottom_exact;
        self.gutter = from.gutter;
        self.mirror_margins = from.mirror_margins;
    }
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
            view: None,
            scripts: crate::props::ScriptSizes::default(),
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
                            + sheet.characters.capacity()
                                * std::mem::size_of::<crate::character_styles::CharacterStyle>()
                            + sheet
                                .characters
                                .iter()
                                .map(|s| s.name.capacity())
                                .sum::<usize>()
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
    /// Removing replies whose annotation went, so the removal's own edits
    /// do not start another round.
    pruning: bool,
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

/// `base` for a run that keeps what it is: a picture or note it stands for,
/// the link it belongs to, the tracked change it is part of and the comment
/// it is about. Reverting to a style resets how text looks, not these.
fn keep_content(base: CharProps, old: &CharProps) -> CharProps {
    CharProps {
        object: old.object,
        link: old.link,
        rev: old.rev,
        comment: old.comment,
        ..base
    }
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
            pruning: false,
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
    pub fn import_fragment_styled(
        &mut self,
        frag: &Fragment,
        fonts: &[String],
        styles: &[String],
        characters: &[crate::character_styles::CharacterStyle],
    ) -> Fragment {
        let mut character_map = vec![0];
        for s in characters {
            character_map.push(self.import_character_style(s, fonts, true));
        }
        let mut out = frag.clone();
        for p in out
            .paras
            .iter_mut()
            .chain(out.notes.iter_mut().flat_map(|note| &mut note.paras))
        {
            let mut map = |c: &mut CharProps| {
                c.character_style = character_map
                    .get(c.character_style as usize)
                    .copied()
                    .unwrap_or(0);
                if c.character_style == 0 {
                    c.direct_format = 0;
                }
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

    /// Whether `match_saved` can make the document `saved` as a step undo
    /// can take back.
    pub fn can_match_saved(&self, saved: &Document) -> bool {
        saved.fonts == self.fonts
            && saved.objects == self.objects
            && saved.headers.len() == self.headers.len()
    }

    /// Make the document what `saved` is, as one undoable step, after a
    /// save that did not keep it exactly. Only the paragraphs that differ
    /// are replaced. Returns false, changing nothing, when the fonts or
    /// pictures differ, since undo cannot put those back.
    pub fn match_saved(&mut self, saved: &Document, label: &'static str, sel: Selection) -> bool {
        if !self.can_match_saved(saved) {
            return false;
        }
        self.commit_open();
        self.begin(label, sel);
        if saved.styles != self.styles {
            self.record_sheet();
            self.styles = saved.styles.clone();
        }
        if saved.section != self.section {
            self.record_section();
            self.section = saved.section.clone();
        }
        self.change_document_props(saved.props.clone());
        let shown = std::mem::take(&mut self.header_section);
        let stories = [StoryId::Main, StoryId::Notes, StoryId::Annotations]
            .into_iter()
            .chain(StoryId::HEADERS.into_iter().take(saved.headers.len()));
        for id in stories {
            let want = match id {
                StoryId::Main => &saved.main,
                StoryId::Notes => &saved.notes,
                StoryId::Annotations => &saved.annotations,
                _ => &saved.headers[id.header_index()],
            };
            let have = &self.story(id).paras;
            let same = |(a, b): &(&Paragraph, &Paragraph)| a == b;
            let front = have.iter().zip(&want.paras).take_while(same).count();
            if front == have.len() && front == want.paras.len() {
                continue;
            }
            let room = have.len().min(want.paras.len()) - front;
            let back = have
                .iter()
                .rev()
                .zip(want.paras.iter().rev())
                .take(room)
                .take_while(same)
                .count();
            let remove = have.len() - front - back;
            let new = want.paras[front..want.paras.len() - back].to_vec();
            self.splice(id, front, remove, new);
        }
        self.header_section = shown;
        self.commit(sel);
        true
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

    /// Take back everything in the open step and drop it, leaving Undo
    /// and Redo as they were before `begin`. For a change that turns out
    /// not to be allowed partway through. A typing step that joined the
    /// last one is taken back whole, so call this only on a step begun
    /// with `begin`.
    pub fn abandon(&mut self) {
        if let Some(step) = self.history.open.take() {
            self.apply_inverse(step);
            self.history.can_coalesce = false;
        }
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
        // A step can change the headers of several sections; the pane
        // stays on the one it showed.
        let shown = self.header_section;
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
        self.header_section = shown;
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
        // Further typing must have its own state, rather than reuse the
        // saved typing step's identity and appear unchanged.
        self.break_typing();
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
        mut new: Vec<Paragraph>,
    ) {
        assert!(
            !new.is_empty() || remove < self.story(story).paras.len(),
            "a story needs a paragraph"
        );
        new.iter_mut().for_each(|p| self.styles.keep_overrides(p));
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
        let removed_annotations = crate::notes::annotation_ids(&old);
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
        let annotations = changed_kinds.contains(&crate::notes::NoteKind::Annotation);
        for kind in changed_kinds {
            self.reconcile(kind);
        }
        if annotations && !self.pruning {
            let present: std::collections::HashSet<u32> =
                crate::notes::annotation_ids(&self.main.paras)
                    .into_iter()
                    .collect();
            let gone: std::collections::HashSet<u32> = removed_annotations
                .into_iter()
                .filter(|id| !present.contains(id))
                .collect();
            if !gone.is_empty() {
                self.pruning = true;
                self.drop_orphan_replies(gone);
                self.pruning = false;
            }
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
        self.split_paragraph_typing(story, at, None)
    }

    /// Return typed at the keyboard: as `split_paragraph`, but a new empty
    /// paragraph that keeps its style also keeps `typing`, the format the
    /// person was typing in, so a font chosen at the insertion point
    /// carries on to the next line instead of falling back to the mark's.
    /// Only the look is taken; the mark keeps its own links, comments and
    /// revision.
    pub fn split_paragraph_typing(
        &mut self,
        story: StoryId,
        at: Position,
        typing: Option<CharProps>,
    ) -> Position {
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
            } else if let Some(chp) = typing {
                let own = tail.mark_chp;
                tail.mark_chp = CharProps {
                    object: own.object,
                    link: own.link,
                    rev: own.rev,
                    comment: own.comment,
                    ..chp
                };
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
            pruning: false,
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
        self.format_chars_with(story, start, end, &mut f);
    }

    // The body takes the closure as a trait object so it is built once, not
    // again for every closure a caller passes (a size saving).
    fn format_chars_with(
        &mut self,
        story: StoryId,
        start: Position,
        end: Position,
        f: &mut dyn FnMut(&mut CharProps),
    ) {
        self.change_chars(story, start, end, true, f);
    }

    /// `format_chars`, where `direct` says the change is formatting by hand
    /// that a named character style must not overwrite.
    fn change_chars(
        &mut self,
        story: StoryId,
        start: Position,
        end: Position,
        direct: bool,
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
            if direct && c.character_style != 0 && before.character_style == c.character_style {
                c.direct_format |= crate::character_styles::CharacterFormat::difference(before, *c);
            }
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
        let hyperlink = if link != 0 {
            self.styles.find_character("Hyperlink").or_else(|| {
                let props = CharProps {
                    color,
                    underline: crate::props::Underline::Single,
                    ..Default::default()
                };
                self.add_character_style(
                    "Hyperlink",
                    crate::character_styles::CharacterFormat::new(props, (1 << 4) | (1 << 11)),
                )
            })
        } else {
            self.styles.find_character("Hyperlink")
        };
        let paras = &self.story(story).paras;
        let mut new = Vec::with_capacity(end.para - start.para + 1);
        for (i, p) in paras.iter().enumerate().take(end.para + 1).skip(start.para) {
            let mut p = p.clone();
            let s = if i == start.para { start.offset } else { 0 };
            let e = if i == end.para { end.offset } else { p.len() };
            let mark = p.mark_chp;
            p.format_range(s, e, &mut |c| {
                if link == 0 {
                    if hyperlink == Some(c.character_style) {
                        c.character_style = 0;
                        c.direct_format = 0;
                    }
                    c.color = mark.color;
                    c.underline = mark.underline;
                } else if let Some(id) = hyperlink {
                    // Keep the previous layer's visible formatting as direct
                    // formatting when the link takes over its named slot.
                    if c.character_style != id {
                        c.character_style = 0;
                        c.direct_format = 0;
                    }
                    *c = self.styles.with_character_style(*c, mark, id);
                    if c.color != color {
                        c.color = color;
                        c.direct_format |= 1 << 11;
                    }
                } else {
                    c.color = color;
                    c.underline = crate::props::Underline::Single;
                }
                c.link = link;
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
        self.format_paras_with(story, first, last, &mut f);
    }

    fn format_paras_with(
        &mut self,
        story: StoryId,
        first: usize,
        last: usize,
        f: &mut dyn FnMut(&mut ParaProps),
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
                let mut restyle = |c: &mut CharProps| {
                    let before = *c;
                    *c = restyle_chp(*c, &old_style.chp, &new_style.chp);
                    self.styles.keep_character_format(before, c, new_style.chp);
                };
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
                // How the paragraph looks goes back to the style's. What it
                // is does not change: its place in a table stays.
                let mut pap = style.pap.clone();
                pap.style = p.pap.style;
                pap.in_table = p.pap.in_table;
                pap.nest = p.pap.nest;
                p.pap = pap;
                let base = style.chp;
                let len = p.len();
                p.format_range(0, len, &mut |c| {
                    *c = keep_content(base, c);
                });
                p.mark_chp = keep_content(base, &p.mark_chp);
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

    /// The width of a column in the section paragraph `para` is in: the
    /// room a table or a paragraph there has.
    pub fn column_width_at(&self, para: usize) -> Twips {
        self.section_props_at(para)
            .column_width(self.props.text_width())
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

    /// Replace Fonts: text and styles in any font of `from` take font `to`.
    /// Call it with a step open. Undo restores the text and styles of the
    /// body, notes, annotations and every section's headers and footers.
    pub fn replace_font(&mut self, from: &[u16], to: u16) {
        self.change_everywhere(0, |c| {
            if from.contains(&c.font) {
                c.font = to;
            }
        });
    }

    /// Every style and all the text take font `to` and size `size`. Call it
    /// with a step open.
    pub fn set_font_everywhere(&mut self, to: u16, size: u16) {
        self.change_everywhere(0, |c| {
            c.font = to;
            c.size = size;
        });
    }

    /// Text and styles in a font that `keep` does not name take font `to`,
    /// and the size of what changes is multiplied by `num / den`. Call it
    /// with a step open.
    pub fn replace_font_scaled(
        &mut self,
        keep: impl Fn(&str) -> bool,
        to: u16,
        num: u32,
        den: u32,
    ) {
        let change: Vec<bool> = self.fonts.iter().map(|name| !keep(name)).collect();
        self.change_everywhere(0, |c| {
            if change.get(usize::from(c.font)).copied().unwrap_or(false) {
                c.font = to;
                c.size = (u32::from(c.size) * num / den.max(1)).clamp(2, 3276) as u16;
            }
        });
    }

    /// Take the newest undo step out of the history without undoing it,
    /// for a change that belongs to the view rather than the document.
    /// The document then counts as modified only if it was before.
    pub fn forget_last_step(&mut self, was_modified: bool) {
        self.commit_open();
        self.history.undo.pop();
        self.history.can_coalesce = false;
        if was_modified {
            self.touch();
        } else {
            self.mark_saved();
        }
    }

    /// Text and styles in color `from` take color `to`, as Replace Fonts
    /// does for fonts: call it with a step open.
    pub fn replace_color(&mut self, from: crate::props::Rgb, to: crate::props::Rgb) {
        self.change_everywhere(11, |c| {
            if c.color == from {
                c.color = to;
            }
        });
    }

    /// Run `fix` over every style, the character styles that set field
    /// `bit`, and all the text, for Replace Fonts and Delete Color.
    fn change_everywhere(&mut self, bit: u16, fix: impl Fn(&mut CharProps)) {
        self.record_sheet();
        for s in &mut self.styles.styles {
            fix(&mut s.chp);
            // A linked style's own formatting comes back when it is
            // unlinked, so it changes too.
            fix(&mut s.own_chp);
        }
        for s in &mut self.styles.characters {
            if s.format.mask & (1 << bit) != 0 {
                fix(&mut s.format.props);
            }
        }
        for (k, story) in self.every_story() {
            self.in_header_section(k, |d| {
                let last = d.story(story).paras.len() - 1;
                let end = Position::new(last, d.story(story).paras[last].len());
                // The definitions changed above, so a styled run still
                // follows its style: this is not formatting by hand.
                d.change_chars(story, Position::new(0, 0), end, false, &fix);
            });
        }
        // The text of notes set aside for a reference that may be pasted
        // back is not in a story, so it changes here.
        for p in self.note_bank.values_mut().flatten() {
            p.runs.iter_mut().for_each(|r| fix(&mut r.chp));
            fix(&mut p.mark_chp);
        }
    }

    /// Every story there is, each once: the body, the notes, the
    /// annotations, and the headers and footers of each section that has
    /// its own (the first always does). A header is named by its section
    /// as well, since `StoryId::Header` alone means the one the header
    /// pane shows. Run a change to one in `in_header_section`.
    pub(crate) fn every_story(&self) -> Vec<(usize, StoryId)> {
        let mut out = vec![
            (0, StoryId::Main),
            (0, StoryId::Notes),
            (0, StoryId::Annotations),
        ];
        for k in 0..self.sections().len() {
            if self.has_own_headers(k) {
                out.extend(StoryId::HEADERS.map(|id| (k, id)));
            }
        }
        out
    }

    /// Run `f` with the header stories standing for section `k`'s, then
    /// put the pane's section back. Changes made in `f` are recorded
    /// against section `k`, so undo puts them back there.
    pub(crate) fn in_header_section<R>(&mut self, k: usize, f: impl FnOnce(&mut Self) -> R) -> R {
        let shown = std::mem::replace(&mut self.header_section, k);
        let out = f(self);
        self.header_section = shown;
        out
    }

    // ----- the style sheet -----

    /// Save the style sheet in the open undo step, before changing it.
    pub(crate) fn record_sheet(&mut self) {
        if self.history.open.is_none() {
            self.begin("Style", Selection::default());
        }
        if self
            .history
            .open
            .as_ref()
            .unwrap()
            .records
            .iter()
            .any(|r| r.sheet.is_some())
        {
            return;
        }
        let sheet = Box::new(self.styles.clone());
        let step = self.history.open.as_mut().unwrap();
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
    /// Give style `id` new formatting. `chp` is the style's own character
    /// formatting; a linked character style stays linked and is laid over it.
    pub fn redefine_style(&mut self, id: StyleId, pap: ParaProps, chp: CharProps) {
        self.record_sheet();
        self.redefine(id, pap, chp, 0);
    }

    /// Redefine style `id` and link it to character style `character`, or
    /// unlink it with zero. `own` is the style's own character formatting.
    pub fn redefine_style_linked(
        &mut self,
        id: StyleId,
        pap: ParaProps,
        own: CharProps,
        character: u16,
    ) {
        if id as usize >= self.styles.styles.len() {
            return;
        }
        self.record_sheet();
        let character = if self.styles.character(character).is_some() {
            character
        } else {
            0
        };
        self.styles.styles[id as usize].character = character;
        self.redefine(id, pap, own, 0);
    }

    /// Bring the paragraph styles linked to character style `character` up to
    /// date after it changed.
    pub(crate) fn relink_paragraph_styles(&mut self, character: u16) {
        for k in 0..self.styles.styles.len() as StyleId {
            let s = self.styles.get(k);
            if s.character == character {
                let (pap, own) = (s.pap.clone(), s.own());
                self.redefine(k, pap, own, 0);
            }
        }
    }

    fn redefine(&mut self, id: StyleId, pap: ParaProps, own: CharProps, depth: u8) {
        let old = self.styles.get(id).clone();
        let mut pap = pap;
        pap.style = id;
        let chp = self.styles.linked_chp(own, old.character);
        let st = &mut self.styles.styles[id as usize];
        st.pap = pap.clone();
        st.own_chp = own;
        st.chp = chp;
        // Restyle the paragraphs, in every story.
        let restyle = |styles: &StyleSheet, p: &mut Paragraph| {
            p.pap = restyle_pap(&p.pap, &old.pap, &pap);
            let len = p.len();
            p.format_range(0, len, &mut |c| {
                let before = *c;
                *c = restyle_chp(*c, &old.chp, &chp);
                styles.keep_character_format(before, c, chp);
            });
            let before = p.mark_chp;
            p.mark_chp = restyle_chp(p.mark_chp, &old.chp, &chp);
            styles.keep_character_format(before, &mut p.mark_chp, chp);
        };
        for (k, story) in self.every_story() {
            self.in_header_section(k, |d| {
                for i in 0..d.story(story).paras.len() {
                    if d.story(story).paras[i].pap.style != id {
                        continue;
                    }
                    let mut p = d.story(story).paras[i].clone();
                    restyle(&d.styles, &mut p);
                    if p != d.story(story).paras[i] {
                        d.splice(story, i, 1, vec![p]);
                    }
                }
            });
        }
        for p in self.note_bank.values_mut().flatten() {
            if p.pap.style == id {
                restyle(&self.styles, p);
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
            // A based-on style follows what this one now looks like.
            let dc = restyle_chp(s.own(), &old.chp, &chp);
            self.redefine(k, dp, dc, depth + 1);
        }
    }

    /// How many paragraphs have style `id`, in every story and in the notes
    /// set aside for deleted references. A note, comment, header or footer
    /// story that is only its one empty paragraph is not counted: it is
    /// there whether or not the document has any.
    pub fn style_uses(&self, id: StyleId) -> usize {
        let count = |paras: &[Paragraph]| paras.iter().filter(|p| p.pap.style == id).count();
        let stories: usize = self
            .every_story()
            .into_iter()
            .map(|(k, story)| {
                let paras = match story {
                    StoryId::Main | StoryId::Notes | StoryId::Annotations => {
                        &self.story(story).paras
                    }
                    _ => &self.header_story(k, story).paras,
                };
                let unused = story != StoryId::Main
                    && matches!(paras.as_slice(), [only] if only.text.is_empty());
                if unused { 0 } else { count(paras) }
            })
            .sum();
        stories + self.note_bank.values().map(|p| count(p)).sum::<usize>()
    }

    /// Delete a style the user defined. Its paragraphs go back to Normal,
    /// in every story: the numbers of the styles after it move down by one,
    /// so a paragraph left alone anywhere would take another style.
    pub fn delete_style(&mut self, id: StyleId) {
        self.delete_style_replacing(id, 0);
    }

    /// Delete a style the user defined, giving its paragraphs style `with`
    /// instead (Normal when `with` is the style itself or not a style).
    pub fn delete_style_replacing(&mut self, id: StyleId, with: StyleId) {
        if id == 0 || self.styles.get(id).builtin || id as usize >= self.styles.styles.len() {
            return;
        }
        let with = if with == id || with as usize >= self.styles.styles.len() {
            0
        } else {
            with
        };
        let stories = self.every_story();
        for &(k, story) in &stories {
            self.in_header_section(k, |d| {
                // apply_style one paragraph at a time keeps hand formatting.
                for i in 0..d.story(story).paras.len() {
                    if d.story(story).paras[i].pap.style == id {
                        d.apply_style(story, i, i, with);
                    }
                }
            });
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
        for &(k, story) in &stories {
            self.in_header_section(k, |d| {
                for i in 0..d.story(story).paras.len() {
                    let s = d.story(story).paras[i].pap.style;
                    if s > id {
                        let mut p = d.story(story).paras[i].clone();
                        p.pap.style = s - 1;
                        d.splice(story, i, 1, vec![p]);
                    }
                }
            });
        }
        // The notes set aside for references that were deleted are not in
        // a story.
        let move_down = |p: &mut Paragraph| {
            if p.pap.style == id {
                p.pap.style = fix(with);
            } else {
                p.pap.style = fix(p.pap.style);
            }
        };
        self.note_bank
            .values_mut()
            .flat_map(|paras| paras.iter_mut())
            .for_each(move_down);
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
        borders
    );
    if (p.shading, p.shading_color) == (old.shading, old.shading_color) {
        out.shading = new.shading;
        out.shading_color = new.shading_color;
    }
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
mod tests;
