//! The text each comment is about, as the .docx and RTF readers settle it.
//!
//! A comment's range comes before its mark in both formats, so the readers
//! can't yet know the comment's number when they reach its text. They give
//! the text a provisional number, `PROVISIONAL` plus the range's place in
//! the file, and `settle` swaps in the comment's own once every mark has
//! been read. Numbering comments by their marks keeps the numbers the same
//! when Quill saves a file and opens it again.

use quill_core::Document;
use quill_core::para::chars;
use std::collections::HashMap;

/// Provisional numbers start here, far above any real note number.
pub const PROVISIONAL: u32 = 0x4000_0000;

/// Give each range's text its comment's number (`real` maps provisional
/// numbers to them), or none when the comment is missing. A reply's text
/// belongs to the comment its thread starts with, as Word gives each reply
/// its own copy of the range. Comment marks are never part of a range.
pub fn settle(doc: &mut Document, real: &HashMap<u32, u32>) {
    let info = doc.annotation_info.clone();
    let top = |mut id: u32| {
        for _ in 0..64 {
            match info.get(&id).and_then(|i| i.parent) {
                Some(p) => id = p,
                None => break,
            }
        }
        id
    };
    for p in &mut doc.main.paras {
        if !p.runs.iter().any(|run| run.chp.comment != 0) {
            continue;
        }
        p.format_range(0, p.len(), &mut |c| {
            if c.comment != 0 {
                c.comment = real
                    .get(&c.comment)
                    .copied()
                    .filter(|id| info.contains_key(id))
                    .map_or(0, top);
            }
        });
        let marks: Vec<usize> = p
            .text
            .match_indices(chars::ANNOTATION)
            .map(|(at, _)| at)
            .collect();
        for at in marks {
            p.format_range(at, at + chars::ANNOTATION.len_utf8(), &mut |c| {
                c.comment = 0
            });
        }
    }
}
