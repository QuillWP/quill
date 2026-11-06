//! The range of every measurement a document holds, kept to what Word
//! itself allows. A file or a paste can say anything, such as a margin of
//! two billion twips or margins wider than the page. Without these limits,
//! sums overflow and layout gets sizes nothing could show. Every reader
//! calls `Document::keep_in_bounds` on what it reads.

use crate::document::{DocumentProps, SectionProps};
use crate::para::Paragraph;
use crate::props::{
    Borders, CharProps, Frame, FrameH, FrameV, LineSpacing, ParaProps, RowHeight, RowProps, Twips,
};
use crate::{Document, Story};

/// 22 inches: Word's largest page, and its largest indent or spacing.
pub const MAX_LENGTH: Twips = 31_680;
/// The smallest page side.
pub const MIN_PAGE: Twips = 1440;
/// Margins always leave this much between them for text: half an inch.
pub const MIN_TEXT: Twips = 720;
/// Font sizes in half-points: 1 to 1638 points, as in Word.
pub const MIN_SIZE: u16 = 2;
pub const MAX_SIZE: u16 = 3276;
/// Word's most columns in a section.
pub const MAX_COLUMNS: u16 = 45;
/// The longest font or style name kept, in characters: Word allows 255
/// for a style and far fewer for a font. A damaged file can have pages
/// of data where a name should be, and every run would repeat it.
pub const MAX_NAME: usize = 255;

/// `name` cut to `MAX_NAME` characters.
fn clip_name(name: &mut String) {
    if let Some((at, _)) = name.char_indices().nth(MAX_NAME) {
        name.truncate(at);
    }
}

/// The largest document Quill opens or saves, counting every story (main
/// text, notes, annotations, headers and footers): about 10,000 pages of
/// paragraphs, Word's own limit of 32 MB of text, and this many changes
/// of character formatting. Every format's reader takes anything this
/// size that Quill writes, so a document Quill opens can always be saved and
/// opened again.
pub const MAX_PARAGRAPHS: usize = 250_000;
pub const MAX_CHARACTERS: usize = 32 << 20;
pub const MAX_RUNS: usize = 1_000_000;

/// How big a document is: its paragraphs, characters and runs of
/// character formatting, across every story.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub paragraphs: usize,
    pub characters: usize,
    pub runs: usize,
}

impl Size {
    /// Why a document this size is too large for Quill, or None if it isn't.
    pub fn too_large(&self) -> Option<String> {
        let over = |n: usize, max: usize, what: &str| {
            (n > max).then(|| format!("it has {n} {what}, and Quill's limit is {max}"))
        };
        over(self.paragraphs, MAX_PARAGRAPHS, "paragraphs")
            .or_else(|| over(self.characters, MAX_CHARACTERS, "characters"))
            .or_else(|| over(self.runs, MAX_RUNS, "changes of formatting"))
    }

    /// The size of plain text, counted without building any paragraphs.
    pub fn of_text(text: &str) -> Size {
        Size {
            paragraphs: text.bytes().filter(|&b| b == b'\n').count() + 1,
            characters: text.chars().count(),
            runs: 1,
        }
    }
}

fn length(v: Twips) -> Twips {
    v.clamp(-MAX_LENGTH, MAX_LENGTH)
}

fn distance(v: Twips) -> Twips {
    v.clamp(0, MAX_LENGTH)
}

/// Shrink `parts` in proportion so that they add up to no more than
/// `room`.
fn fit(parts: &mut [&mut Twips], room: Twips) {
    let total: i64 = parts.iter().map(|p| i64::from(**p)).sum();
    if total <= i64::from(room) || total == 0 {
        return;
    }
    for p in parts.iter_mut() {
        **p = (i64::from(**p) * i64::from(room.max(0)) / total) as Twips;
    }
}

impl DocumentProps {
    /// Page sizes and margins within range, with room left for text.
    pub fn keep_in_bounds(&mut self) {
        self.page_width = self.page_width.clamp(MIN_PAGE, MAX_LENGTH);
        self.page_height = self.page_height.clamp(MIN_PAGE, MAX_LENGTH);
        for m in [
            &mut self.margin_left,
            &mut self.margin_right,
            &mut self.margin_top,
            &mut self.margin_bottom,
            &mut self.gutter,
        ] {
            *m = distance(*m);
        }
        fit(
            &mut [
                &mut self.margin_left,
                &mut self.margin_right,
                &mut self.gutter,
            ],
            self.page_width - MIN_TEXT,
        );
        fit(
            &mut [&mut self.margin_top, &mut self.margin_bottom],
            self.page_height - MIN_TEXT,
        );
        self.default_tab = self.default_tab.clamp(36, MAX_LENGTH);
    }
}

impl SectionProps {
    pub fn keep_in_bounds(&mut self) {
        self.columns = self.columns.clamp(1, MAX_COLUMNS);
        self.line_count_by = self.line_count_by.max(1);
        for d in [
            &mut self.column_spacing,
            &mut self.header_from_top,
            &mut self.footer_from_bottom,
            &mut self.page_number_from_top,
            &mut self.page_number_from_right,
        ] {
            *d = distance(*d);
        }
        if let Some(d) = &mut self.line_from_text {
            *d = distance(*d);
        }
    }
}

impl CharProps {
    pub fn keep_in_bounds(&mut self) {
        self.size = self.size.clamp(MIN_SIZE, MAX_SIZE);
        self.position = length(self.position);
        self.spacing = length(self.spacing);
        // A format change's earlier format comes back on Reject, so it
        // gets the same treatment.
        if let Some(r) = crate::revision::get(self.rev)
            && let Some((_, old)) = &r.formatted
        {
            let mut fixed = *old;
            fixed.size = fixed.size.clamp(MIN_SIZE, MAX_SIZE);
            fixed.position = length(fixed.position);
            fixed.spacing = length(fixed.spacing);
            if fixed != *old {
                self.rev = crate::revision::update(self.rev, |r| {
                    if let Some(f) = &mut r.formatted {
                        f.1 = fixed;
                    }
                });
            }
        }
    }
}

fn borders(b: &mut Borders) {
    for side in [
        &mut b.top,
        &mut b.bottom,
        &mut b.left,
        &mut b.right,
        &mut b.between,
    ] {
        side.space = distance(side.space);
    }
}

fn frame(f: &mut Frame) {
    if let FrameH::At(x) = &mut f.h {
        *x = length(*x);
    }
    if let FrameV::At(y) = &mut f.v {
        *y = length(*y);
    }
    if let Some(w) = &mut f.width {
        *w = (*w).clamp(1, MAX_LENGTH);
    }
    f.from_text = distance(f.from_text);
}

impl ParaProps {
    pub fn keep_in_bounds(&mut self) {
        self.left = length(self.left);
        self.right = length(self.right);
        self.first = length(self.first);
        self.before = distance(self.before);
        self.after = distance(self.after);
        match &mut self.line {
            LineSpacing::AtLeast(t) | LineSpacing::Exactly(t) => *t = distance(*t),
            LineSpacing::Auto | LineSpacing::Multiple(_) => {}
        }
        for t in &mut self.tabs {
            t.pos = distance(t.pos);
        }
        self.tabs.sort_by_key(|t| t.pos);
        self.tabs.dedup_by_key(|t| t.pos);
        borders(&mut self.borders);
        if let Some(f) = &mut self.frame {
            frame(f);
        }
    }
}

impl RowProps {
    pub fn keep_in_bounds(&mut self) {
        self.gap = distance(self.gap);
        self.indent = length(self.indent);
        match &mut self.height {
            RowHeight::AtLeast(t) | RowHeight::Exactly(t) => *t = distance(*t),
            RowHeight::Auto => {}
        }
        for c in &mut self.cells {
            c.width = distance(c.width);
            borders(&mut c.borders);
        }
    }
}

impl Paragraph {
    pub fn keep_in_bounds(&mut self) {
        self.pap.keep_in_bounds();
        self.mark_chp.keep_in_bounds();
        let len = self.len();
        self.format_range(0, len, &mut CharProps::keep_in_bounds);
        if let Some(row) = &mut self.row {
            row.keep_in_bounds();
        }
        if let Some(s) = &mut self.section {
            s.keep_in_bounds();
            if let Some(headers) = &mut s.headers {
                std::sync::Arc::make_mut(headers)
                    .iter_mut()
                    .for_each(Story::keep_in_bounds);
            }
        }
    }
}

impl Story {
    pub fn keep_in_bounds(&mut self) {
        self.paras.iter_mut().for_each(Paragraph::keep_in_bounds);
    }
}

impl Document {
    pub fn size(&self) -> Size {
        let section_headers = self
            .main
            .paras
            .iter()
            .filter_map(|p| p.section.as_ref()?.headers.as_ref())
            .flat_map(|h| h.iter());
        let stories = [&self.main, &self.notes, &self.annotations]
            .into_iter()
            .chain(&self.headers)
            .chain(section_headers);
        let mut size = Size::default();
        for p in stories.flat_map(|s| &s.paras) {
            size.paragraphs += 1;
            size.characters += p.text.chars().count();
            size.runs += p.runs.len();
        }
        size
    }

    /// Bring every measurement in the document within range: the page,
    /// sections, styles, and each paragraph and run of every story.
    pub fn keep_in_bounds(&mut self) {
        self.props.keep_in_bounds();
        self.section.keep_in_bounds();
        self.fonts.iter_mut().for_each(clip_name);
        for s in &mut self.styles.styles {
            clip_name(&mut s.name);
            s.pap.keep_in_bounds();
            s.chp.keep_in_bounds();
        }
        for story in [&mut self.main, &mut self.notes, &mut self.annotations] {
            story.keep_in_bounds();
        }
        self.headers.iter_mut().for_each(Story::keep_in_bounds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_and_style_names_are_cut_to_a_sensible_length() {
        let mut doc = Document::new();
        doc.fonts[0] = "é".repeat(10_000);
        doc.styles.styles[0].name = "x".repeat(300);
        doc.keep_in_bounds();
        assert_eq!(doc.fonts[0].chars().count(), MAX_NAME);
        assert!(doc.fonts[0].chars().all(|c| c == 'é'));
        assert_eq!(doc.styles.styles[0].name.len(), MAX_NAME);
    }

    #[test]
    fn margins_wider_than_the_page_shrink_to_leave_room_for_text() {
        let mut p = DocumentProps {
            page_width: 15840,
            margin_left: 8640,
            margin_right: 8640,
            ..Default::default()
        };
        p.keep_in_bounds();
        assert_eq!(p.margin_left, p.margin_right);
        assert!(p.text_width() >= MIN_TEXT);
        // A sensible page is left alone.
        let mut ok = DocumentProps::default();
        let before = ok.clone();
        ok.keep_in_bounds();
        assert_eq!(ok, before);
    }

    #[test]
    fn absurd_numbers_come_back_within_range() {
        let mut doc = Document::new();
        let p = &mut doc.main.paras[0];
        p.pap.first = i32::MIN;
        p.pap.left = i32::MAX;
        p.pap.line = LineSpacing::Exactly(-5);
        p.mark_chp.size = 0;
        p.insert(
            0,
            "x",
            CharProps {
                size: u16::MAX,
                position: i32::MAX,
                ..Default::default()
            },
        );
        doc.props.page_width = i32::MAX;
        doc.props.default_tab = 0;
        doc.section.columns = 0;
        doc.section.line_count_by = 0;
        doc.keep_in_bounds();
        let p = &doc.main.paras[0];
        assert_eq!((p.pap.first, p.pap.left), (-MAX_LENGTH, MAX_LENGTH));
        assert_eq!(p.pap.line, LineSpacing::Exactly(0));
        assert_eq!(p.mark_chp.size, MIN_SIZE);
        let chp = p.chp_of_char(0);
        assert_eq!((chp.size, chp.position), (MAX_SIZE, MAX_LENGTH));
        assert_eq!(doc.props.page_width, MAX_LENGTH);
        assert_eq!(doc.props.default_tab, 36);
        assert_eq!((doc.section.columns, doc.section.line_count_by), (1, 1));
    }
}
