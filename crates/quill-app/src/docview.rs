//! A document window's text area in Normal view: layout cache, drawing,
//! selection, and the commands that edit the text.

use crate::canvas::{Canvas, Rect, Surface, color};
use crate::commands::Cmd;
use quill_core::document::{Fragment, StoryId};
use quill_core::para::{Mark, Paragraph, chars};
use quill_core::props::{Align, BorderStyle, CharProps, LineSpacing, ParaProps, Underline};
use quill_core::table;
use quill_core::{Document, Position, Selection};
use quill_layout::layout::{Kind, RuleKind};
use quill_layout::paginate::{self, Keeps, PageStart};
use quill_layout::{
    BorderBox, Fonts, GlyphCache, Joins, LayoutOptions, ParaLayout, border_width,
    layout_paragraph_joined,
};
use std::time::{Duration, Instant};

mod cells;
mod changes;
mod comment_cards;
pub use cells::CELL_ALIGNMENTS;
mod clipboard;
mod compose;
mod contents;
mod drawing;
mod editing;
mod frames;
mod language;
mod layout_engine;
mod link;
mod lists;
mod live_spelling;
mod markdown;
mod memory;
pub use language::HyphenSpot;
mod borders;
mod notes;
mod outline;
mod picture;
mod sections;
mod table_move;
mod tools;

pub use borders::BorderKind;
pub use clipboard::{Clip, Clipboard};
pub use comment_cards::{LINK, dotted_line};
pub(crate) use drawing::{draw_border_box, paint_paragraph};
pub use link::LinkTarget;
pub use picture::Handle;

const MAIN: StoryId = StoryId::Main;

/// Space between the left edge of the text area and the left margin of the
/// text. The selection bar lives in it.
pub const TEXT_LEFT: f32 = 12.0;
/// Gray between pages in Page Layout view, and above the first.
pub(crate) const PAGE_GAP: f32 = 36.0;
/// Space above the first line.
const TEXT_TOP: f32 = 6.0;
/// Normal view's room above a section break mark, in points: enough for
/// its label between the section's last line and the next paragraph.
const SECTION_PAD: f32 = 14.0;
/// How long the caret stays on and off.
pub const BLINK: Duration = Duration::from_millis(530);
/// Clicks closer together than this make double and triple clicks.
const MULTI_CLICK: Duration = Duration::from_millis(450);

/// Font sizes that Larger/Smaller Font Size step through.
const SIZE_STEPS: [u16; 11] = [7, 9, 10, 12, 14, 18, 24, 36, 48, 60, 72];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragUnit {
    Char,
    Word,
    Para,
    Line,
}

/// Something a command asks the application to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    NewWindow,
    Open,
    Close,
    Save,
    SaveAs,
    Quit,
    About,
    /// Help > Licenses…: every license Quill carries.
    Licenses,
    /// Format > Replace Missing Fonts…, which needs the installed fonts.
    ReplaceFonts,
    OpenRecent(usize),
    RemoveRecent(usize),
    ClearRecent,
    Dialog(DialogId),
    NativePreview,
    Print,
    SavePdf,
    /// View > Markdown > Source and WYSIWYG.
    MarkdownSource,
    MarkdownWysiwyg,
    /// Search again for what Find last looked for.
    FindAgain,
    /// Split the window into two panes, or join them.
    Split,
    /// Open another window on the same document.
    Mirror,
    /// Insert > Picture: choose a file.
    InsertPicture,
    /// Open a story in the pane: View > Footnotes.
    Pane(StoryId),
    /// Open the footnote or annotation pane at a note.
    OpenNote(StoryId, u32),
    /// Print Merge Helper with no data document yet: choose one.
    ChooseMergeData,
    /// Reply to the annotation at the insertion point, or resolve it (or
    /// open it again). The window finds which: it may be in the pane.
    ReplyToAnnotation,
    ResolveAnnotation,
    /// Delete the annotation here and its replies.
    DeleteAnnotation,
    /// Fill the screen with the window, or put it back.
    FullScreen,
    /// Zoom in (1) or out (-1) a step, or back to 100% (0).
    Zoom(i8),
    LookUp,
}

/// What each window on a shared document keeps for itself: where it is
/// scrolled and what is selected in it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Port {
    pub scroll_y: f32,
    pub scroll_x: f32,
    pub sel: Selection,
    pub upstream: bool,
}

/// Dialogs a command can open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogId {
    InsertTable,
    PasteSpecial,
    PrintMerge,
    MergeHelper,
    Spelling,
    Hyphenation,
    Thesaurus,
    Footnote,
    Toc,
    Index,
    TableCells,
    TableLayout,
    PageSetup,
    Print,
    Character,
    Paragraph,
    SummaryInfo,
    Section,
    Document,
    Border,
    Style,
    CharacterStyles,
    Preferences,
    DefaultFont,
    ToolbarPreferences,
    Find,
    Replace,
    GoTo,
    Grep,
    RegexSearch,
    Sed,
    Glossary,
    WordCount,
    ChangeCase,
    Renumber,
    ShowClipboard,
    Frame,
    DropCap,
    Hyperlink,
    Picture,
    ReviewChanges,
}

/// A run of the column: one paragraph, or a table row.
#[derive(Clone, Debug)]
struct Block {
    first: usize,
    last: usize,
    top: f32,
    height: f32,
    row: Option<RowGeom>,
}

/// Where a table row's cells sit, in points from the left margin.
#[derive(Clone, Debug)]
struct RowGeom {
    /// Each cell: its first and last paragraph and its left and right
    /// boundary.
    cells: Vec<(usize, usize, f32, f32)>,
    props: quill_core::RowProps,
    /// The rows of tables in this row's cells, at any depth.
    nested: Vec<NestedRow>,
    /// Cells merged with the cell under them, or the one above, which
    /// draw no line between.
    joined_below: Vec<bool>,
    joined_above: Vec<bool>,
}

/// A row of a table inside a cell: where it sits, in points from the left
/// margin and the top of the text.
#[derive(Clone, Debug)]
struct NestedRow {
    /// Its first paragraph and its end mark.
    first: usize,
    last: usize,
    /// How many tables deep it is: 2 for a table in a cell.
    level: u8,
    top: f32,
    height: f32,
    geom: RowGeom,
}

/// Remembered for Edit > Repeat.
#[derive(Clone, Debug)]
enum Repeatable {
    Command(Cmd),
    Text(String),
}

pub struct DocView {
    pub(crate) doc: Document,
    pub title: String,
    /// Where the document was opened from or last saved to.
    pub path: Option<std::path::PathBuf>,
    /// The file's name and format were chosen in Save As, so Save may
    /// write there directly even if it is not a .qwp file.
    pub path_chosen: bool,
    /// The file's modified time and size when it was opened or last saved,
    /// so Save can tell when something else changed it since.
    pub disk_stamp: Option<(std::time::SystemTime, u64)>,
    pub sel: Selection,
    /// At a line wrap, show the caret at the end of the upper line.
    pub upstream: bool,
    pub show_marks: bool,
    /// Show tracked changes marked up (View > Show Changes), or the text
    /// as it would be with every change accepted.
    pub show_changes: bool,
    /// This is the annotations pane: marks show who wrote each one.
    pub captions: bool,
    /// For the comments pane: how large its own text is drawn compared with
    /// its nominal size, so the document's zoom does not enlarge comments.
    pub comment_scale: f32,
    /// Format for text typed at the insertion point after a formatting
    /// command with nothing selected.
    typing_chp: Option<CharProps>,
    /// What `typing_chp` was before each formatting command at the
    /// insertion point, so Undo can take those back before anything is
    /// typed.
    caret_formats: Vec<Option<CharProps>>,
    goal_x: Option<f32>,
    layouts: Vec<Option<ParaLayout>>,
    /// Keep every paragraph's glyphs (printing): see `memory`.
    pub keep_all: bool,
    /// The paragraphs last drawn, first and past the last: a few ranges,
    /// newest first, for the two panes of a split window and Print Preview.
    drawn: Vec<(usize, usize)>,
    spelling: live_spelling::LiveSpelling,
    tops: Vec<f32>,
    /// Where each paragraph's text starts across, from the left margin:
    /// 0 outside tables, the cell's text edge inside them.
    xs: Vec<f32>,
    /// The width each paragraph was laid out at.
    widths: Vec<f32>,
    /// Paragraphs and table rows down the column, for finding and drawing.
    blocks: Vec<Block>,
    laid_out_width: f32,
    /// Whether hidden text was laid out, so a change re-lays everything.
    laid_out_hidden: bool,
    /// The same for tracked changes.
    laid_out_changes: bool,
    /// Where each page starts, from the last repagination.
    pub pages: Vec<PageStart>,
    /// Each column's share of the text and where it goes on its page.
    bands: Vec<paginate::Band>,
    /// Extra top and bottom room claimed by tall headers and footers on
    /// pages whose corresponding document margin is At Least.
    page_body_extra: Vec<(f32, f32)>,
    /// Each section's first paragraph and settings, when last laid out.
    secs: Vec<(usize, quill_core::SectionProps)>,
    /// The footnotes' paragraphs laid out, the width of each, and which
    /// paragraphs each note has.
    note_layouts: Vec<ParaLayout>,
    note_widths: Vec<f32>,
    note_ranges: std::collections::HashMap<u32, std::ops::Range<usize>>,
    /// What each footnote reference shows, by note id.
    note_labels: std::sync::Arc<std::collections::HashMap<u32, String>>,
    /// The initials in each annotation mark.
    annotation_labels: std::sync::Arc<std::collections::HashMap<u32, String>>,
    notes_dirty: bool,
    /// Notes gathered at the ends of sections, placed in the text.
    endnotes: Vec<notes::EndNotes>,
    /// Outline view, which paragraphs it has collapsed out of sight, and
    /// its display options.
    pub outline: bool,
    collapsed: Vec<bool>,
    outline_dirty: bool,
    /// An empty document opened in Outline: its one paragraph shows as a
    /// heading and becomes Heading 1 when typed in, so merely looking
    /// leaves the document unchanged.
    outline_start: bool,
    /// Collapsed flags for paragraphs just moved, from the first one.
    collapse_after: Option<(usize, Vec<bool>)>,
    pub first_line_only: bool,
    pub show_formatting: bool,
    /// ⌘⌥T was pressed: the next key is an outline command.
    pub outline_prefix: bool,
    /// A table of contents just made, to make again with its own pages.
    toc_refresh: Option<quill_core::contents::TocOptions>,
    /// The pages were worked out again, so a live table of contents may
    /// need catching up; and how many times running it has just done so.
    toc_stale: bool,
    toc_rounds: u8,
    /// Hyphenate All under way.
    hyph_job: Option<language::HyphenJob>,
    /// Page height, top and bottom margins and widow control when the
    /// pages were worked out, to repaginate when they change.
    laid_out_page: (i32, i32, i32, bool),
    /// Page Layout view: pages drawn as sheets, text placed on them.
    pub page_view: bool,
    /// View > Markdown > Source: the formatted document the text stands for.
    pub md_source: Option<Box<markdown::MarkdownSource>>,
    /// What Markdown cannot carry that the person has already been warned of
    /// on saving as Markdown, so Save does not ask again.
    pub md_warned: Vec<&'static str>,
    /// Lay out headers and footers even outside Page Layout view (for
    /// Print Preview and printing).
    pub headers_wanted: bool,
    /// A copy laid out for paper: hidden text only if the document says
    /// to print it, and no Show ¶ marks.
    printing: bool,
    /// Header and footer layouts for Page Layout view, by story and page
    /// number (0 when the story has no page number field, so one layout
    /// serves every page).
    hf: std::collections::HashMap<(StoryId, usize, u32), Vec<ParaLayout>>,
    hf_dirty: bool,
    /// What the headers in `hf` were laid out with, besides their text:
    /// when it changes they are laid out again.
    hf_basis: Option<layout_engine::HfBasis>,
    /// The page number and its format each header in `hf` was laid out
    /// with, for the ones that show it.
    hf_numbers:
        std::collections::HashMap<(StoryId, usize, u32), (u32, quill_core::PageNumberFormat)>,
    pub scroll_y: f32,
    pub scroll_x: f32,
    /// The text area on screen, set each frame.
    pub area: Rect,
    caret_since: Instant,
    /// What an input method is composing at the insertion point.
    composing: Option<compose::Composing>,
    drag: Option<(DragUnit, Selection)>,
    last_click: Option<(Instant, f32, f32, u32)>,
    repeat: Option<Repeatable>,
    /// A message shown where the page number goes, like Calculate's
    /// answer. Cleared by the next edit or move.
    pub status: Option<String>,
    /// Where the last few edits were, newest last, for Go Back.
    back: Vec<Position>,
    /// A picture handle being dragged.
    pic_drag: Option<picture::PicDrag>,
    /// The picture Crop in Place is on for.
    pic_crop: Option<Position>,
    /// Control is held, so a picture's handles crop.
    pub crop_held: bool,
    table_drag: Option<table_move::TableDrag>,
    /// A cell border being dragged.
    border_drag: Option<borders::BorderDrag>,
    /// Frames placed on the pages (Page Layout view and printing).
    frames: Vec<frames::FrameBox>,
    /// The areas each paragraph's lines were laid out to keep clear of,
    /// and the areas the placed frames now ask for.
    wraps: Vec<Vec<quill_layout::Clear>>,
    want_wrap: Vec<Vec<quill_layout::Clear>>,
    /// Whether frames were placed (rather than left in the text) when the
    /// paragraphs were last placed.
    placed_frames_on: bool,
    /// Normal view's room for section break marks: where each starts,
    /// down the text. Page counting leaves it out (see `unpad`).
    section_pads: Vec<f32>,
    /// The view the paragraphs were last placed for: Normal view leaves
    /// room for section marks, Page Layout does not.
    placed_page_view: bool,
}

impl DocView {
    pub fn new(doc: Document, title: String) -> Self {
        DocView {
            doc,
            title,
            path: None,
            path_chosen: false,
            disk_stamp: None,
            sel: Selection::default(),
            upstream: false,
            show_marks: false,
            show_changes: true,
            captions: false,
            comment_scale: 1.0,
            typing_chp: None,
            caret_formats: Vec::new(),
            goal_x: None,
            layouts: Vec::new(),
            keep_all: false,
            drawn: Vec::new(),
            spelling: live_spelling::LiveSpelling::default(),
            tops: Vec::new(),
            xs: Vec::new(),
            widths: Vec::new(),
            blocks: Vec::new(),
            laid_out_width: 0.0,
            laid_out_hidden: false,
            laid_out_changes: true,
            pages: Vec::new(),
            bands: Vec::new(),
            page_body_extra: Vec::new(),
            secs: Vec::new(),
            note_layouts: Vec::new(),
            note_widths: Vec::new(),
            note_ranges: Default::default(),
            note_labels: Default::default(),
            annotation_labels: Default::default(),
            notes_dirty: true,
            endnotes: Vec::new(),
            outline: false,
            collapsed: Vec::new(),
            outline_dirty: false,
            outline_start: false,
            collapse_after: None,
            first_line_only: false,
            show_formatting: true,
            outline_prefix: false,
            toc_refresh: None,
            toc_stale: false,
            toc_rounds: 0,
            hyph_job: None,
            laid_out_page: (0, 0, 0, false),
            page_view: false,
            md_source: None,
            md_warned: Vec::new(),
            headers_wanted: false,
            printing: false,
            hf: std::collections::HashMap::new(),
            hf_dirty: true,
            hf_basis: None,
            hf_numbers: std::collections::HashMap::new(),
            scroll_y: 0.0,
            scroll_x: 0.0,
            area: Rect::default(),
            caret_since: Instant::now(),
            composing: None,
            drag: None,
            last_click: None,
            repeat: None,
            status: None,
            back: Vec::new(),
            pic_drag: None,
            pic_crop: None,
            crop_held: false,
            table_drag: None,
            border_drag: None,
            frames: Vec::new(),
            wraps: Vec::new(),
            want_wrap: Vec::new(),
            placed_frames_on: false,
            section_pads: Vec::new(),
            placed_page_view: false,
        }
    }

    /// Turn an owned view into a print view without cloning its document.
    /// Call `update_layout` before drawing it. Background PDF jobs already
    /// own a stable document snapshot.
    pub fn into_print(mut self) -> DocView {
        self.printing = true;
        self.headers_wanted = true;
        self
    }

    /// Scroll Page Layout view to the top of page `k`.
    pub fn show_page(&mut self, k: usize) {
        self.scroll_y = (self.page_top(k) - PAGE_GAP / 2.0).max(0.0);
    }

    /// The page (from 0) the insertion point is on.
    pub fn caret_page(&self) -> usize {
        if self.pages.is_empty() || self.layouts.is_empty() {
            return 0;
        }
        let (_, top, _) = self.caret_geometry(self.sel.focus, self.upstream);
        paginate::page_at(&self.pages, top)
    }

    /// The pages (from 1) holding a table wider than its column in a
    /// section of several columns, reaching past the space between columns
    /// so it runs over the text beside it. Quill lays a table out at its own
    /// width, so these pages may not look as the document's author meant.
    /// Needs the layout.
    pub fn wide_table_pages(&self) -> Vec<usize> {
        let doc = &self.doc;
        let text_width = doc.props.text_width();
        let mut pages: Vec<usize> = Vec::new();
        if self.pages.is_empty() || self.layouts.is_empty() {
            return pages;
        }
        for (k, range) in doc.sections().into_iter().enumerate() {
            let s = doc.nth_section(k);
            if s.columns < 2 {
                continue;
            }
            let n = i32::from(s.columns);
            let column = (text_width - s.column_spacing * (n - 1)) / n;
            for i in range {
                let p = &doc.main.paras[i];
                let Some(row) = &p.row else {
                    continue;
                };
                if p.pap.nest != 0 {
                    continue;
                }
                let right = row.indent + row.cells.iter().map(|c| c.width).sum::<i32>();
                if right <= column + s.column_spacing {
                    continue;
                }
                let (_, top, _) = self.caret_geometry(Position::new(i, 0), false);
                let page = paginate::page_at(&self.pages, top) + 1;
                if pages.last() != Some(&page) {
                    pages.push(page);
                }
            }
        }
        pages
    }

    /// Page at the top of the viewport, independent of the insertion point.
    pub fn visible_page(&self) -> usize {
        if self.page_view {
            let (_, height) = self.page_size();
            (self.scroll_y / (height + PAGE_GAP)).floor().max(0.0) as usize
        } else {
            paginate::page_at(&self.pages, self.scroll_y)
        }
        .min(self.pages.len().saturating_sub(1))
    }

    /// Select `sel`, as a dialog does after changing the text.
    pub fn set_selection(&mut self, sel: Selection) {
        self.sel = sel;
        self.clamp_selection();
        self.confine_to_comment();
        self.goal_x = None;
        self.typing_chp = None;
        self.reset_blink();
    }

    /// Trade this window's scroll position and selection for another
    /// window's, when the document moves between windows.
    pub fn swap_port(&mut self, port: &mut Port) {
        std::mem::swap(&mut self.scroll_y, &mut port.scroll_y);
        std::mem::swap(&mut self.scroll_x, &mut port.scroll_x);
        std::mem::swap(&mut self.sel, &mut port.sel);
        std::mem::swap(&mut self.upstream, &mut port.upstream);
        self.clamp_selection();
        self.goal_x = None;
        self.drag = None;
        self.doc.break_typing();
    }

    /// Keep the selection inside the text after it was replaced underneath.
    pub fn clamp_selection(&mut self) {
        let story = &self.doc.main;
        let fix = |p: Position| {
            let para = p.para.min(story.paras.len() - 1);
            Position::new(para, p.offset.min(story.paras[para].len()))
        };
        self.sel = Selection::new(fix(self.sel.anchor), fix(self.sel.focus));
    }

    /// Re-lay out everything, for example after Show ¶ changes hidden text.
    pub fn invalidate_layout(&mut self) {
        self.layouts.iter_mut().for_each(|l| *l = None);
    }

    /// Replace document-wide settings without letting callers forget undo,
    /// modified-state tracking, or layout invalidation.
    pub fn set_document_props(&mut self, label: &'static str, props: quill_core::DocumentProps) {
        if self.doc.set_document_props(label, props, self.sel) {
            self.invalidate_layout();
        }
    }

    /// Change document-wide settings through the same guarded path.
    pub fn edit_document_props(
        &mut self,
        label: &'static str,
        edit: impl FnOnce(&mut quill_core::DocumentProps),
    ) {
        let mut props = self.doc.props.clone();
        edit(&mut props);
        self.set_document_props(label, props);
    }

    /// Replace summary metadata as one undoable action.
    pub fn set_summary(&mut self, summary: quill_core::SummaryInfo) {
        self.doc.set_summary(summary, self.sel);
    }

    /// Replace the final section's settings and invalidate dependent layout.
    pub fn set_last_section(&mut self, label: &'static str, section: quill_core::SectionProps) {
        if self.doc.set_last_section(label, section, self.sel) {
            self.invalidate_layout();
        }
    }

    fn layout(&self, para: usize) -> &ParaLayout {
        self.layouts[para].as_ref().expect("layout is up to date")
    }

    pub fn total_height(&self) -> f32 {
        if self.page_view {
            let (_, ph) = self.page_size();
            return PAGE_GAP + self.pages.len().max(1) as f32 * (ph + PAGE_GAP);
        }
        self.tops.last().copied().unwrap_or(0.0) + TEXT_TOP + 24.0
    }

    // ----- coordinates -----
    //
    // Text is laid out in one continuous column ("document" coordinates:
    // x from the left margin, y down the column). The view places it: in
    // Normal view the column runs down the window; in Page Layout view
    // each page's share sits inside that page's margins. "View"
    // coordinates are the scrollable content's, before scrolling.

    /// Page size in points.
    pub fn page_size(&self) -> (f32, f32) {
        let p = &self.doc.props;
        (p.page_width as f32 / 20.0, p.page_height as f32 / 20.0)
    }

    /// View x of a page's left edge: centered when the window is wide enough.
    fn page_left(&self) -> f32 {
        let (pw, _) = self.page_size();
        ((self.area.w - pw) / 2.0).max(PAGE_GAP).floor() - self.scroll_x
    }

    /// View y of page `k`'s top edge.
    fn page_top(&self, k: usize) -> f32 {
        let (_, ph) = self.page_size();
        PAGE_GAP + k as f32 * (ph + PAGE_GAP)
    }

    /// View x of the text's left margin.
    fn text_x(&self) -> f32 {
        if self.page_view {
            let p = &self.doc.props;
            self.page_left() + (p.margin_left + p.gutter) as f32 / 20.0
        } else {
            TEXT_LEFT - self.scroll_x
        }
    }

    /// The window x of the left text margin, for the ruler.
    pub fn text_left_window(&self) -> f32 {
        self.area.x + self.text_x()
    }

    /// View y of document y `y`.
    fn to_view(&self, y: f32) -> f32 {
        if !self.page_view || self.pages.is_empty() {
            return TEXT_TOP + y;
        }
        let (k, _, local) = self.on_page(y);
        self.page_top(k) + self.page_body_top(k) + local
    }

    /// The document point at view point (`x` from the left margin, `vy`).
    fn doc_at(&self, x: f32, vy: f32) -> (f32, f32) {
        if !self.page_view || self.pages.is_empty() {
            return (x, vy - TEXT_TOP);
        }
        let (_, ph) = self.page_size();
        let k = (((vy - PAGE_GAP / 2.0) / (ph + PAGE_GAP)).floor().max(0.0) as usize)
            .min(self.pages.len() - 1);
        let local = vy - self.page_top(k) - self.page_body_top(k);
        self.doc_at_page(k, x, local)
    }

    /// Window position of document point (x, y).
    fn to_window(&self, x: f32, y: f32) -> (f32, f32) {
        let dx = if self.page_view {
            self.on_page(y).1
        } else {
            0.0
        };
        (
            self.area.x + self.text_x() + x + dx,
            self.area.y - self.scroll_y + self.to_view(y),
        )
    }

    /// Caret position for `pos` in document coordinates: x, line top, height.
    fn caret_geometry(&self, pos: Position, upstream: bool) -> (f32, f32, f32) {
        let l = self.layout(pos.para);
        let (li, x) = l.caret(pos.offset, upstream);
        let line = &l.lines[li];
        (
            self.xs[pos.para] + x,
            self.tops[pos.para] + line.top,
            line.height,
        )
    }

    /// The text position under a point in window coordinates.
    pub fn hit(&self, wx: f32, wy: f32) -> (Position, bool) {
        let (x, y) = self.doc_at(
            wx - self.area.x - self.text_x(),
            wy - self.area.y + self.scroll_y,
        );
        self.hit_doc(x, y)
    }

    /// The text position at document point (x, y).
    fn hit_doc(&self, x: f32, y: f32) -> (Position, bool) {
        if y < 0.0 {
            return (Position::new(0, 0), false);
        }
        let n = self.layouts.len();
        if let Some(para) = self.frame_at(x, y) {
            let l = self.layout(para);
            let li = l.line_at_y(y - self.tops[para]);
            let (offset, upstream) = l.hit(li, x - self.xs[para]);
            return (Position::new(para, offset), upstream);
        }
        let bi = self.block_at(y);
        let b = &self.blocks[bi];
        let para = match &b.row {
            None => b.first,
            Some(row) => {
                // The cell under x (or the nearest), then its paragraph.
                let k = row
                    .cells
                    .iter()
                    .position(|c| x < c.3)
                    .unwrap_or(row.cells.len() - 1);
                let (mut a, mut last, _, _) = row.cells[k];
                // Under the top of a vertical merge: the top cell, which
                // holds the text.
                if row.joined_above.get(k).copied().unwrap_or(false) {
                    let left = row.cells[k].2;
                    for above in self.blocks[..bi].iter().rev() {
                        let Some(r) = &above.row else {
                            break;
                        };
                        let Some(j) = r.cells.iter().position(|c| (c.2 - left).abs() < 1.0) else {
                            break;
                        };
                        (a, last) = (r.cells[j].0, r.cells[j].1);
                        if !r.joined_above.get(j).copied().unwrap_or(false) {
                            break;
                        }
                    }
                }
                // A table in that cell, under the point: its cell instead,
                // and so on inwards.
                let mut level = 1;
                while let Some(inner) = row.nested.iter().find(|n| {
                    n.level == level + 1
                        && n.first >= a
                        && n.last <= last
                        && n.top <= y
                        && y < n.top + n.height
                }) {
                    let cells = &inner.geom.cells;
                    let k = cells
                        .iter()
                        .position(|c| x < c.3)
                        .unwrap_or(cells.len() - 1);
                    (a, last) = (cells[k].0, cells[k].1);
                    level += 1;
                }
                (a..=last).rev().find(|&p| self.tops[p] <= y).unwrap_or(a)
            }
        };
        let l = self.layout(para);
        let ly = y - self.tops[para];
        if ly > l.height && para == n - 1 {
            let end = self.doc.main.end();
            return (end, false);
        }
        let li = l.line_at_y(ly);
        let (offset, upstream) = l.hit(li, x - self.xs[para]);
        (Position::new(para, offset), upstream)
    }

    // ----- scrolling -----

    pub fn max_scroll_x(&self) -> f32 {
        let width = if self.page_view {
            self.page_size().0 + 2.0 * PAGE_GAP
        } else {
            self.laid_out_width + TEXT_LEFT + 24.0
        };
        (width - self.area.w).max(0.0)
    }

    pub fn scroll_sideways(&mut self, dx: f32) {
        if dx.is_finite() {
            self.scroll_x = (self.scroll_x + dx).clamp(0.0, self.max_scroll_x());
        }
    }

    pub fn scroll_by(&mut self, dy: f32) {
        let max = (self.total_height() - self.area.h).max(0.0);
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, max);
    }

    pub fn max_scroll(&self) -> f32 {
        (self.total_height() - self.area.h).max(0.0)
    }

    /// Scroll so the middle of the window stays on the same spot of the
    /// document when the zoom goes from `old` to `new`. The area is still
    /// the old zoom's: the window is the same size, so it covers
    /// `old / new` as much of the document afterwards.
    pub fn zoom_around_center(&mut self, old: f32, new: f32) {
        if !(old > 0.0 && new > 0.0 && old.is_finite() && new.is_finite()) || self.area.h <= 0.0 {
            return;
        }
        let ratio = old / new;
        self.scroll_y = (self.scroll_y + self.area.h / 2.0 * (1.0 - ratio)).max(0.0);
        if self.page_view {
            let (pw, _) = self.page_size();
            let centered = |w: f32| ((w - pw) / 2.0).max(PAGE_GAP).floor();
            let (before, after) = (self.area.w, self.area.w * ratio);
            // How far the middle of the window is from the page's left edge.
            let from_edge = before / 2.0 - (centered(before) - self.scroll_x);
            let most = (pw + 2.0 * PAGE_GAP - after).max(0.0);
            self.scroll_x = (centered(after) - after / 2.0 + from_edge).clamp(0.0, most);
        }
    }

    /// Scroll so the selection's moving end is on screen.
    pub fn scroll_to_caret(&mut self) {
        if self.layouts.iter().any(Option::is_none) || self.area.h <= 0.0 {
            return;
        }
        // Geometry excludes scrolling. Reveal the moving endpoint horizontally too.
        let (x, y, h) = self.caret_geometry(self.sel.focus, self.upstream);
        self.scroll_sideways(0.0);
        let visible_x = self.to_window(x, y).0;
        if visible_x < self.area.x + 8.0 {
            self.scroll_sideways(visible_x - self.area.x - 8.0);
        } else if visible_x > self.area.right() - 8.0 {
            self.scroll_sideways(visible_x - self.area.right() + 8.0);
        }
        let y = self.to_view(y);
        let top = self.scroll_y;
        let view = self.area.h;
        if y < top {
            self.scroll_y = (y - view / 3.0).max(0.0);
        } else if y + h > top + view {
            self.scroll_y = y + h - view * 2.0 / 3.0;
        }
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
    }

    // ----- mouse -----

    /// A secondary click keeps a selection when invoked inside it and never
    /// participates in the primary button's double/triple-click sequence.
    pub fn context_at(&mut self, pos: Position, upstream: bool) {
        let (start, end) = self.sel.range();
        let inside = match self.selected_cells() {
            Some(_) => self.in_selected_cells(pos),
            None => !self.sel.is_empty() && start <= pos && pos < end,
        };
        if !inside {
            self.sel = Selection::caret(pos);
            self.upstream = upstream;
            self.typing_chp = None;
            self.goal_x = None;
            self.doc.break_typing();
        }
        self.last_click = None;
        self.drag = None;
        self.reset_blink();
    }

    pub fn mouse_down(&mut self, wx: f32, wy: f32, shift: bool, cmd: bool) {
        self.mouse_down_inner(wx, wy, shift, cmd);
        self.confine_to_comment();
    }

    fn mouse_down_inner(&mut self, wx: f32, wy: f32, shift: bool, cmd: bool) {
        self.status = None;
        let now = Instant::now();
        let count = match self.last_click {
            Some((t, x, y, n))
                if now - t < MULTI_CLICK && (x - wx).abs() < 4.0 && (y - wy).abs() < 4.0 =>
            {
                n + 1
            }
            _ => 1,
        };
        self.last_click = Some((now, wx, wy, count));
        if count == 1
            && (self.picture_mouse_down(wx, wy, cmd)
                || self.table_mouse_down(wx, wy)
                || self.border_mouse_down(wx, wy))
        {
            return;
        }
        if self.outline {
            let (x, y) = self.doc_at(
                wx - self.area.x - self.text_x(),
                wy - self.area.y + self.scroll_y,
            );
            if let Some(i) = self.symbol_at(x, y) {
                self.drag = None;
                self.symbol_click(i, count >= 2);
                return;
            }
        }
        self.goal_x = None;
        self.typing_chp = None;
        self.doc.break_typing();
        let (pos, upstream) = self.hit(wx, wy);
        let in_bar = !self.captions && wx < self.area.x + self.text_x() - 2.0;
        // A live table of contents is one object: a click selects it all.
        if count == 1
            && !shift
            && !cmd
            && self.doc.props.live_toc.is_some()
            && let Some(end) = self.doc.existing_toc()
            && pos.para <= end
        {
            self.sel = Selection::new(Position::new(0, 0), Position::new(end + 1, 0));
            self.drag = None;
            self.status = Some("Table of contents: right-click for options".into());
            self.reset_blink();
            return;
        }
        // ⌘-click on a link follows it, as in today's Word; anywhere else
        // it selects the sentence, as in the original.
        if cmd
            && !shift
            && !in_bar
            && count == 1
            && let Some(url) = self.link_url_near(pos)
        {
            self.open_link(&url);
            self.drag = None;
            return;
        }
        let story = &self.doc.main;
        let (unit, range) = if in_bar {
            if cmd {
                (DragUnit::Line, (story.start(), story.end()))
            } else if count >= 2 {
                (DragUnit::Para, story.paragraph_range(pos.para))
            } else {
                (DragUnit::Line, self.line_range(pos, upstream))
            }
        } else if cmd {
            (DragUnit::Word, story.sentence_at(pos))
        } else {
            // Decided 2026-10-30 (Lucas): dragging after a double or triple
            // click grows the selection a character at a time, not a word or
            // paragraph at a time; the word or paragraph stays selected.
            match count {
                1 => (DragUnit::Char, (pos, pos)),
                2 => (DragUnit::Char, story.word_at(pos)),
                _ => (DragUnit::Char, story.paragraph_range(pos.para)),
            }
        };
        if shift {
            let anchor = self.sel.anchor;
            let focus = if range.0 < anchor { range.0 } else { range.1 };
            self.sel = Selection::new(anchor, focus);
            self.drag = Some((unit, Selection::caret(anchor)));
        } else {
            self.sel = Selection::new(range.0, range.1);
            self.drag = Some((unit, self.sel));
        }
        self.upstream = upstream && self.sel.is_empty();
        self.reset_blink();
    }

    pub fn mouse_drag(&mut self, wx: f32, wy: f32) {
        self.mouse_drag_inner(wx, wy);
        self.confine_to_comment();
    }

    fn mouse_drag_inner(&mut self, wx: f32, wy: f32) {
        if self.picture_drag(wx, wy)
            || self.table_mouse_drag(wx, wy)
            || self.border_mouse_drag(wx, wy)
        {
            return;
        }
        let Some((unit, origin)) = self.drag else {
            return;
        };
        let (pos, upstream) = self.hit(wx, wy);
        let story = &self.doc.main;
        let (s, e) = match unit {
            DragUnit::Char => (pos, pos),
            DragUnit::Word => story.word_at(pos),
            DragUnit::Para => story.paragraph_range(pos.para),
            DragUnit::Line => self.line_range(pos, upstream),
        };
        let (os, oe) = origin.range();
        self.sel = if s < os {
            Selection::new(oe, s)
        } else {
            Selection::new(os, e.max(oe))
        };
        self.upstream = upstream && self.sel.is_empty();
        self.reset_blink();
    }

    pub fn mouse_up(&mut self) {
        self.drag = None;
        self.picture_mouse_up();
        self.table_mouse_up();
        self.border_mouse_up();
        self.confine_to_comment();
    }

    /// Whether a picture's handle is being dragged.
    pub fn picture_dragging(&self) -> bool {
        self.pic_drag.is_some()
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
            || self.pic_drag.is_some()
            || self.table_drag.is_some()
            || self.border_drag.is_some()
    }

    /// The whole line around `pos`, as clicking in the selection bar picks.
    fn line_range(&self, pos: Position, upstream: bool) -> (Position, Position) {
        let l = self.layout(pos.para);
        let li = l.line_of(pos.offset, upstream);
        let line = &l.lines[li];
        let start = Position::new(pos.para, line.start);
        let end = if li + 1 == l.lines.len() {
            self.doc
                .main
                .next_char(Position::new(pos.para, self.doc.main.paras[pos.para].len()))
        } else {
            Position::new(pos.para, line.end)
        };
        (start, end)
    }
}

/// A page number as Section > Page Numbers > Format writes it.
/// Layout options for the fields on page `page`: its number, and today's
/// date and time.
pub fn fields(page: u32) -> LayoutOptions {
    let now = crate::clock::now();
    LayoutOptions {
        page_number: page,
        date: now.date(),
        time: now.time(),
        ..LayoutOptions::default()
    }
}

#[cfg(test)]
mod tests;
