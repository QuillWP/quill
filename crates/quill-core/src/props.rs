//! Character and paragraph formatting.
//!
//! Lengths are twips (1/20 point) and font sizes are half-points, the same
//! units Word and .docx use, so nothing is lost on a round trip.

/// A length in twips, 1/20 of a point. 1440 twips make an inch.
pub type Twips = i32;

pub const TWIPS_PER_INCH: Twips = 1440;

/// Index into the document's font table.
pub type FontId = u16;

/// Index into the document's style sheet.
pub type StyleId = u16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Underline {
    #[default]
    None,
    Single,
    /// Underlines words but not the spaces between them.
    Word,
    Double,
    Dotted,
}

/// An RGB color packed as 0xRRGGBB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb(pub u32);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0x000000);
    pub const WHITE: Rgb = Rgb(0xFFFFFF);

    pub fn r(self) -> u8 {
        (self.0 >> 16) as u8
    }
    pub fn g(self) -> u8 {
        (self.0 >> 8) as u8
    }
    pub fn b(self) -> u8 {
        self.0 as u8
    }
}

/// The eight colors of the original's Character dialog, in menu order.
pub const WORD_COLORS: [(&str, Rgb); 8] = [
    ("Black", Rgb(0x000000)),
    ("Blue", Rgb(0x0000FF)),
    ("Cyan", Rgb(0x00FFFF)),
    ("Green", Rgb(0x00FF00)),
    ("Magenta", Rgb(0xFF00FF)),
    ("Red", Rgb(0xFF0000)),
    ("Yellow", Rgb(0xFFFF00)),
    ("White", Rgb(0xFFFFFF)),
];

/// Formatting of a run of characters. Small and `Copy` so runs can hold it
/// directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CharProps {
    pub font: FontId,
    /// Size in half-points: 24 is 12 pt.
    pub size: u16,
    pub bold: bool,
    pub italic: bool,
    pub underline: Underline,
    pub strike: bool,
    pub outline: bool,
    pub shadow: bool,
    pub small_caps: bool,
    pub all_caps: bool,
    pub hidden: bool,
    pub color: Rgb,
    /// Raise (positive) or lower (negative) the baseline, in twips.
    pub position: Twips,
    /// Expand (positive) or condense (negative) letter spacing, in twips.
    pub spacing: Twips,
    /// Non-zero when the run is an embedded object (picture, field,
    /// footnote reference). The text holds one U+FFFC per object.
    pub object: u32,
    /// Non-zero when the run is part of a hyperlink: its number in
    /// `crate::link`.
    pub link: u32,
    /// Non-zero when the run is a tracked change: its number in
    /// `crate::revision`.
    pub rev: u32,
    /// Non-zero when the run is part of the text an annotation (a
    /// comment) is about: the annotation's id. Where comments overlap,
    /// the innermost one.
    pub comment: u32,
}

impl Default for CharProps {
    fn default() -> Self {
        CharProps {
            font: 0,
            size: 24,
            bold: false,
            italic: false,
            underline: Underline::None,
            strike: false,
            outline: false,
            shadow: false,
            small_caps: false,
            all_caps: false,
            hidden: false,
            color: Rgb::BLACK,
            position: 0,
            spacing: 0,
            object: 0,
            link: 0,
            rev: 0,
            comment: 0,
        }
    }
}

impl CharProps {
    /// Size in points, rounded down for odd half-points.
    pub fn points(&self) -> u16 {
        self.size / 2
    }

    /// Remove bold, italic, underline and the other style flags, as Plain
    /// Text (⌘⇧Z) does. Font, size and color stay.
    pub fn clear_styles(&mut self) {
        self.bold = false;
        self.italic = false;
        self.underline = Underline::None;
        self.strike = false;
        self.outline = false;
        self.shadow = false;
        self.small_caps = false;
        self.all_caps = false;
        self.hidden = false;
        self.position = 0;
        self.spacing = 0;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// The original's "Line" setting in the Paragraph dialog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineSpacing {
    /// As tall as the tallest thing on the line.
    #[default]
    Auto,
    /// At least this tall, taller if something on the line needs it.
    AtLeast(Twips),
    /// Exactly this tall, clipping anything taller.
    Exactly(Twips),
    /// A multiple of the auto height in 240ths (240 = single). The original has
    /// no such setting but .docx does, so documents from elsewhere keep it.
    Multiple(u16),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TabAlign {
    #[default]
    Left,
    Center,
    Right,
    Decimal,
    /// Draws a vertical bar at the position instead of aligning text.
    Bar,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TabLeader {
    #[default]
    None,
    Dots,
    Dashes,
    Underline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TabStop {
    /// Distance from the left margin, like Word.
    pub pos: Twips,
    pub align: TabAlign,
    pub leader: TabLeader,
}

/// Line styles from the Border dialog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderStyle {
    #[default]
    None,
    Dotted,
    Hairline,
    Single,
    Thick,
    Double,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BorderLine {
    pub style: BorderStyle,
    /// Gap between the line and the text, in twips.
    pub space: Twips,
    /// Black also stands for "automatic" in the file formats that have it.
    pub color: Rgb,
}

impl BorderLine {
    pub fn is_none(&self) -> bool {
        self.style == BorderStyle::None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Borders {
    pub top: BorderLine,
    pub bottom: BorderLine,
    pub left: BorderLine,
    pub right: BorderLine,
    /// Line between consecutive bordered paragraphs.
    pub between: BorderLine,
    /// Word's "Shadow" preset: a box with a heavier bottom and right edge.
    pub shadow: bool,
}

impl Borders {
    pub fn is_none(&self) -> bool {
        self.top.is_none()
            && self.bottom.is_none()
            && self.left.is_none()
            && self.right.is_none()
            && self.between.is_none()
    }
}

/// One cell of a table row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct CellProps {
    /// From the cell's left boundary to the next one's, the gap between
    /// columns included.
    pub width: Twips,
    pub borders: Borders,
    /// In tenths of a percent.
    pub shading: u16,
    /// An explicit solid background. When present, it takes precedence over shading.
    pub shading_color: Option<Rgb>,
    /// Where the cell's text sits when the row is taller than it.
    pub valign: VAlign,
}

/// A cell's vertical alignment (Format > Table Cells).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

/// How tall a table row is (Format > Table Cells > Height).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RowHeight {
    #[default]
    Auto,
    AtLeast(Twips),
    Exactly(Twips),
}

/// A table row's layout. Like Word, Quill keeps it on the row's end mark.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RowProps {
    pub cells: Vec<CellProps>,
    /// Space between columns: half of it pads each side of a cell's text.
    pub gap: Twips,
    /// Where the first cell's text starts, from the left margin.
    pub indent: Twips,
    pub height: RowHeight,
    /// Left, centered or right between the margins.
    pub align: Align,
}

/// The original's Space Between Columns: 0.11 in.
pub const TABLE_GAP: Twips = 158;

impl RowProps {
    /// `cols` cells of `width` each, Word's defaults otherwise.
    pub fn even(cols: usize, width: Twips) -> RowProps {
        RowProps {
            cells: vec![
                CellProps {
                    width,
                    ..Default::default()
                };
                cols.max(1)
            ],
            gap: TABLE_GAP,
            indent: 0,
            height: RowHeight::Auto,
            align: Align::Left,
        }
    }

    pub fn width(&self) -> Twips {
        self.cells.iter().map(|c| c.width).sum()
    }

    /// Each cell's left boundary, from the left margin, for a text column
    /// `text_width` wide.
    pub fn lefts(&self, text_width: Twips) -> Vec<Twips> {
        let start = match self.align {
            Align::Center => (text_width - self.width()) / 2,
            Align::Right => text_width - self.width() + self.gap / 2,
            _ => self.indent - self.gap / 2,
        };
        let mut x = start;
        self.cells
            .iter()
            .map(|c| {
                let l = x;
                x += c.width;
                l
            })
            .collect()
    }
}

/// Paragraph formatting. Lives on the paragraph mark, as in Word.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParaProps {
    pub style: StyleId,
    pub align: Align,
    /// Left indent from the left margin.
    pub left: Twips,
    /// Right indent from the right margin.
    pub right: Twips,
    /// First line indent relative to `left`; negative for a hanging indent.
    pub first: Twips,
    pub before: Twips,
    pub after: Twips,
    pub line: LineSpacing,
    /// Sorted by position.
    pub tabs: Vec<TabStop>,
    pub page_break_before: bool,
    pub keep_with_next: bool,
    pub keep_together: bool,
    pub suppress_line_numbers: bool,
    pub borders: Borders,
    /// Shading in tenths of a percent (125 = 12.5%), 0 for none.
    pub shading: u16,
    /// An explicit solid background. When present, it takes precedence over shading.
    pub shading_color: Option<Rgb>,
    /// The paragraph is inside a table cell. Word marks every paragraph of
    /// a cell this way; the cell's last paragraph ends in a cell mark.
    pub in_table: bool,
    /// How many tables deep the paragraph is inside its first one: 0 in an
    /// ordinary table cell, 1 in a table inside a cell, and so on. Only
    /// means something when `in_table` is set. This is Word's table depth
    /// (RTF `\itap`) less one, kept on each paragraph rather than as a tree.
    pub nest: u8,
    /// Format > Frame: the paragraph is placed on the page and the text
    /// flows round it. Neighboring paragraphs with the same frame share
    /// it.
    pub frame: Option<Frame>,
}

/// Where a frame sits across the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FrameH {
    #[default]
    Left,
    Center,
    Right,
    /// Left on right-hand pages, right on left-hand ones; Outside the
    /// reverse.
    Inside,
    Outside,
    /// This far from the left of what it is relative to.
    At(Twips),
}

/// Where a frame sits down the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FrameV {
    /// Where the paragraph would be, with the text flowing beside it.
    #[default]
    InLine,
    Top,
    Center,
    Bottom,
    /// This far down from the top of what it is relative to.
    At(Twips),
}

/// What a frame's position is measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FrameRel {
    #[default]
    Column,
    Margin,
    Page,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Frame {
    pub h: FrameH,
    pub h_rel: FrameRel,
    pub v: FrameV,
    pub v_rel: FrameRel,
    /// None for Auto: as wide as what is in it.
    pub width: Option<Twips>,
    /// Space between the frame and the text round it.
    pub from_text: Twips,
    /// For a drop cap, how many lines it spans (0 otherwise); .docx keeps
    /// this.
    pub drop_lines: u8,
    /// A drop cap in the margin rather than dropped into the text.
    pub in_margin: bool,
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            h: FrameH::Left,
            h_rel: FrameRel::Column,
            v: FrameV::InLine,
            v_rel: FrameRel::Margin,
            width: None,
            from_text: 0,
            drop_lines: 0,
            in_margin: false,
        }
    }
}

impl Default for ParaProps {
    fn default() -> Self {
        ParaProps {
            style: 0,
            align: Align::Left,
            left: 0,
            right: 0,
            first: 0,
            before: 0,
            after: 0,
            line: LineSpacing::Auto,
            tabs: Vec::new(),
            page_break_before: false,
            keep_with_next: false,
            keep_together: false,
            suppress_line_numbers: false,
            borders: Borders::default(),
            shading: 0,
            shading_color: None,
            in_table: false,
            nest: 0,
            frame: None,
        }
    }
}

impl ParaProps {
    /// How many tables deep the paragraph is: 0 outside any table, 1 in an
    /// ordinary cell, 2 in a table inside a cell.
    pub fn level(&self) -> u8 {
        if self.in_table { self.nest + 1 } else { 0 }
    }

    /// Put the paragraph `level` tables deep (0 takes it out of tables).
    pub fn set_level(&mut self, level: u8) {
        self.in_table = level > 0;
        self.nest = level.saturating_sub(1);
    }

    /// Add or replace a tab stop, keeping the list sorted.
    pub fn set_tab(&mut self, tab: TabStop) {
        self.tabs.retain(|t| t.pos != tab.pos);
        let at = self.tabs.partition_point(|t| t.pos < tab.pos);
        self.tabs.insert(at, tab);
    }

    pub fn clear_tab(&mut self, pos: Twips) {
        self.tabs.retain(|t| t.pos != pos);
    }
}

/// Shading percentages offered by the Border dialog, in tenths of a percent.
pub const SHADING_STEPS: [u16; 17] = [
    0, 100, 125, 200, 250, 300, 375, 400, 500, 600, 625, 700, 750, 800, 875, 900, 1000,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_stay_sorted_and_unique() {
        let mut p = ParaProps::default();
        for pos in [2880, 720, 1440, 720] {
            p.set_tab(TabStop {
                pos,
                ..Default::default()
            });
        }
        let at: Vec<_> = p.tabs.iter().map(|t| t.pos).collect();
        assert_eq!(at, [720, 1440, 2880]);
        p.clear_tab(1440);
        assert_eq!(p.tabs.len(), 2);
    }

    #[test]
    fn default_text_is_12_point() {
        assert_eq!(CharProps::default().points(), 12);
    }
}
