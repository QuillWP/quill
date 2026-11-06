//! Quill's document model and editing engine. No UI, no files: just the
//! text, its formatting, and every edit the original can make to them.

pub mod character_styles;
pub mod contents;
pub mod document;
pub mod find;
pub mod hyphen;
pub mod limits;
pub mod link;
pub mod merge;
pub mod notes;
pub mod para;
pub mod picture;
pub mod props;
pub mod revision;
pub mod spell;
pub mod story;
pub mod styles;
pub mod table;
pub mod text;
pub mod thesaurus;
pub mod tools;
pub mod tracking;

pub use document::{
    Change, Document, DocumentProps, Fragment, LineNumbering, PageNumberFormat, SectionProps,
    SectionStart, StoryId, SummaryInfo,
};
pub use link::Link;
pub use para::{Mark, Paragraph, Run};
pub use picture::{Crop, ImageFormat, Object, Picture};
pub use props::{
    Align, CellProps, CharProps, Frame, FrameH, FrameRel, FrameV, LineSpacing, ParaProps, Rgb,
    RowHeight, RowProps, TABLE_GAP, TabAlign, TabLeader, TabStop, Twips, Underline,
};
pub use story::{Position, Selection, Story};
pub use styles::{Style, StyleSheet};
