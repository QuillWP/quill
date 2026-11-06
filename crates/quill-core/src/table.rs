//! Tables the original way: no separate table object, just paragraphs
//! marked as being in a table. A cell's last paragraph ends with a cell
//! mark; after a row's cells comes an empty paragraph with the row's end
//! mark, which carries the row's cell widths and settings. Every edit here
//! goes through `splice`, so tables undo like everything else.

use crate::document::{Document, StoryId};
use crate::para::{Mark, Paragraph, chars};
use crate::props::{RowProps, Twips};
use crate::story::Position;

/// One row: its paragraphs from `first` to its end mark at `end`, and each
/// cell's first and last paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub first: usize,
    pub end: usize,
    pub cells: Vec<(usize, usize)>,
}

impl Row {
    /// The cell holding paragraph `p`.
    pub fn cell_of(&self, p: usize) -> Option<usize> {
        self.cells.iter().position(|&(a, b)| (a..=b).contains(&p))
    }
}

/// The row holding paragraph `p`, if it is in a table.
pub fn row_at(paras: &[Paragraph], p: usize) -> Option<Row> {
    if !paras.get(p)?.pap.in_table {
        return None;
    }
    let mut first = p;
    while first > 0 && paras[first - 1].pap.in_table && paras[first - 1].mark != Mark::RowEnd {
        first -= 1;
    }
    let mut end = p;
    while end < paras.len() && paras[end].mark != Mark::RowEnd {
        if !paras[end].pap.in_table {
            return None;
        }
        end += 1;
    }
    if end >= paras.len() {
        return None;
    }
    let mut cells = Vec::new();
    let mut start = first;
    for (i, para) in paras.iter().enumerate().take(end).skip(first) {
        if para.mark == Mark::CellEnd {
            cells.push((start, i));
            start = i + 1;
        }
    }
    // Stray paragraphs before the row mark belong to the last cell.
    if start < end {
        match cells.last_mut() {
            Some(c) => c.1 = end - 1,
            None => cells.push((start, end - 1)),
        }
    }
    Some(Row { first, end, cells })
}

/// Every row of the table holding paragraph `p`, top to bottom.
pub fn table_rows(paras: &[Paragraph], p: usize) -> Vec<Row> {
    let Some(row) = row_at(paras, p) else {
        return Vec::new();
    };
    let mut first = row.first;
    while first > 0 && paras[first - 1].mark == Mark::RowEnd {
        match row_at(paras, first - 1) {
            Some(r) => first = r.first,
            None => break,
        }
    }
    let mut rows = Vec::new();
    let mut i = first;
    while let Some(r) = row_at(paras, i) {
        i = r.end + 1;
        rows.push(r);
    }
    rows
}

/// A row's settings: those on its end mark (with a cell per cell), or even
/// columns across a text column `text_width` wide.
pub fn row_props(paras: &[Paragraph], row: &Row, text_width: Twips) -> RowProps {
    let n = row.cells.len().max(1);
    match &paras[row.end].row {
        Some(r) => {
            let mut r = (**r).clone();
            let last = r
                .cells
                .last()
                .cloned()
                .unwrap_or_else(|| RowProps::even(1, text_width / n as Twips).cells[0].clone());
            r.cells.resize(n, last);
            r
        }
        None => RowProps::even(n, text_width / n as Twips),
    }
}

/// How Insert Table turns selected text into cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Convert {
    /// Each paragraph becomes a cell, so many to a row.
    Paragraphs(usize),
    /// Each paragraph becomes a row; tabs separate the cells.
    Tabs,
    /// Each paragraph becomes a row; commas separate the cells.
    Commas,
    /// All the paragraphs side by side in one row.
    SideBySide,
}

impl Document {
    /// An empty cell like those around paragraph `like`.
    fn empty_cell(&self, story: StoryId, like: usize) -> Paragraph {
        let src = &self.story(story).paras[like];
        let mut pap = src.pap.clone();
        pap.in_table = true;
        let mut p = Paragraph::new(pap, src.mark_chp);
        p.mark = Mark::CellEnd;
        p
    }

    fn row_mark(&self, story: StoryId, like: usize, props: RowProps) -> Paragraph {
        let mut p = self.empty_cell(story, like);
        p.mark = Mark::RowEnd;
        p.row = Some(Box::new(props));
        p
    }

    /// Insert > Table: `rows` by `cols` empty cells `width` wide at `at`,
    /// splitting the paragraph there. Returns the first cell's position,
    /// or `at` unchanged inside a table (tables do not nest in the original).
    pub fn insert_table(
        &mut self,
        story: StoryId,
        at: Position,
        rows: usize,
        cols: usize,
        width: Twips,
    ) -> Position {
        if self.story(story).paras[at.para].pap.in_table || rows == 0 || cols == 0 {
            return at;
        }
        let p = if at.offset > 0 {
            self.split_paragraph(story, at).para
        } else {
            at.para
        };
        let mut new = Vec::new();
        for _ in 0..rows {
            for _ in 0..cols {
                new.push(self.empty_cell(story, p));
            }
            new.push(self.row_mark(story, p, RowProps::even(cols, width)));
        }
        self.splice(story, p, 0, new);
        Position::new(p, 0)
    }

    /// Insert > Table with text selected: turn paragraphs `first..=last`
    /// into a table, `width` per column. Returns the first cell.
    pub fn convert_to_table(
        &mut self,
        story: StoryId,
        first: usize,
        last: usize,
        how: Convert,
        width_for: impl Fn(usize) -> Twips,
    ) -> Position {
        let paras = &self.story(story).paras;
        if paras[first..=last].iter().any(|p| p.pap.in_table) {
            return Position::new(first, 0);
        }
        let cell = |p: &Paragraph, a: usize, b: usize| {
            let mut c = p.slice(a, b);
            c.pap.in_table = true;
            c.mark = Mark::CellEnd;
            c.row = None;
            c
        };
        let split = |p: &Paragraph, sep: char| {
            let mut out = Vec::new();
            let mut a = 0;
            for (i, ch) in p.text.char_indices() {
                if ch == sep {
                    out.push(cell(p, a, i));
                    a = i + ch.len_utf8();
                }
            }
            out.push(cell(p, a, p.len()));
            out
        };
        let rows: Vec<Vec<Paragraph>> = match how {
            Convert::Tabs | Convert::Commas => {
                let sep = if how == Convert::Tabs {
                    chars::TAB
                } else {
                    ','
                };
                paras[first..=last].iter().map(|p| split(p, sep)).collect()
            }
            Convert::Paragraphs(cols) => paras[first..=last]
                .chunks(cols.max(1))
                .map(|chunk| chunk.iter().map(|p| cell(p, 0, p.len())).collect())
                .collect(),
            Convert::SideBySide => vec![
                paras[first..=last]
                    .iter()
                    .map(|p| cell(p, 0, p.len()))
                    .collect(),
            ],
        };
        let cols = rows.iter().map(Vec::len).max().unwrap_or(1);
        let width = width_for(cols);
        let mut new = Vec::new();
        for mut row in rows {
            while row.len() < cols {
                row.push(self.empty_cell(story, first));
            }
            new.extend(row);
            new.push(self.row_mark(story, first, RowProps::even(cols, width)));
        }
        // A table never ends the story: the original keeps a paragraph after it.
        if last + 1 == self.story(story).paras.len() {
            let src = &self.story(story).paras[last];
            let mut after = Paragraph::new(src.pap.clone(), src.mark_chp);
            after.pap.in_table = false;
            new.push(after);
        }
        self.splice(story, first, last - first + 1, new);
        Position::new(first, 0)
    }

    /// Change a row's settings.
    pub fn set_row(&mut self, story: StoryId, end: usize, props: RowProps) {
        let mut p = self.story(story).paras[end].clone();
        if p.row.as_deref() == Some(&props) {
            return;
        }
        p.row = Some(Box::new(props));
        self.splice(story, end, 1, vec![p]);
    }

    /// Insert `count` empty rows like `row`, above or below it.
    pub fn insert_rows(
        &mut self,
        story: StoryId,
        row: &Row,
        below: bool,
        count: usize,
        props: &RowProps,
    ) {
        let mut new = Vec::new();
        for _ in 0..count {
            for &(a, _) in &row.cells {
                new.push(self.empty_cell(story, a));
            }
            new.push(self.row_mark(story, row.end, props.clone()));
        }
        let at = if below { row.end + 1 } else { row.first };
        self.splice(story, at, 0, new);
    }

    /// Delete whole rows `first.first..=last.end`. The story keeps its
    /// paragraph after the table.
    pub fn delete_rows(&mut self, story: StoryId, first: &Row, last: &Row) {
        self.splice(story, first.first, last.end - first.first + 1, Vec::new());
    }

    /// Insert `count` empty cells of `width` before cell `at` of a row (at
    /// the end when `at` is the number of cells).
    pub fn insert_cells(
        &mut self,
        story: StoryId,
        row: &Row,
        props: &RowProps,
        at: usize,
        count: usize,
        width: Twips,
    ) {
        let like = row
            .cells
            .get(at)
            .or(row.cells.last())
            .map_or(row.first, |c| c.0);
        let mut new: Vec<Paragraph> = (0..count).map(|_| self.empty_cell(story, like)).collect();
        let mut props = props.clone();
        let cell = crate::props::CellProps {
            width,
            ..props
                .cells
                .get(at.min(props.cells.len().saturating_sub(1)))
                .cloned()
                .unwrap_or_default()
        };
        for _ in 0..count {
            props.cells.insert(at.min(props.cells.len()), cell.clone());
        }
        let pos = row.cells.get(at).map_or(row.end, |c| c.0);
        // Rebuild from the insertion point to the row mark in one edit.
        let paras = &self.story(story).paras;
        let mut tail: Vec<Paragraph> = paras[pos..row.end].to_vec();
        new.append(&mut tail);
        let mut mark = paras[row.end].clone();
        mark.row = Some(Box::new(props));
        new.push(mark);
        self.splice(story, pos, row.end - pos + 1, new);
    }

    /// Delete cells `from..=to` of a row; the row goes too if none are
    /// left.
    pub fn delete_cells(
        &mut self,
        story: StoryId,
        row: &Row,
        props: &RowProps,
        from: usize,
        to: usize,
    ) {
        let to = to.min(row.cells.len() - 1);
        if from == 0 && to + 1 == row.cells.len() {
            self.delete_rows(story, row, row);
            return;
        }
        let paras = &self.story(story).paras;
        let mut new: Vec<Paragraph> = Vec::new();
        for (k, &(a, b)) in row.cells.iter().enumerate() {
            if k < from || k > to {
                new.extend_from_slice(&paras[a..=b]);
            }
        }
        let mut props = props.clone();
        props.cells.drain(from..=to.min(props.cells.len() - 1));
        let mut mark = paras[row.end].clone();
        mark.row = Some(Box::new(props));
        new.push(mark);
        self.splice(story, row.first, row.end - row.first + 1, new);
    }

    /// Format > Table Layout > Merge Cells: cells `from..=to` of a row
    /// become one cell as wide as them all, holding their text as
    /// separate paragraphs (empty cells add nothing).
    pub fn merge_cells(
        &mut self,
        story: StoryId,
        row: &Row,
        props: &RowProps,
        from: usize,
        to: usize,
    ) {
        let to = to.min(row.cells.len() - 1);
        if to <= from {
            return;
        }
        let paras = &self.story(story).paras;
        let mut merged: Vec<Paragraph> = Vec::new();
        for &(a, b) in &row.cells[from..=to] {
            let empty = a == b && paras[a].text.is_empty();
            if empty && !merged.is_empty() {
                continue;
            }
            if let Some(last) = merged.last_mut() {
                last.mark = Mark::Paragraph;
            }
            merged.extend_from_slice(&paras[a..=b]);
        }
        if let Some(last) = merged.last_mut() {
            last.mark = Mark::CellEnd;
        }
        let mut new: Vec<Paragraph> = Vec::new();
        for (k, &(a, b)) in row.cells.iter().enumerate() {
            if k == from {
                new.append(&mut merged);
            } else if k < from || k > to {
                new.extend_from_slice(&paras[a..=b]);
            }
        }
        let mut props = props.clone();
        let width: Twips = props.cells[from..=to.min(props.cells.len() - 1)]
            .iter()
            .map(|c| c.width)
            .sum();
        props.cells.drain(from + 1..=to.min(props.cells.len() - 1));
        props.cells[from].width = width;
        let mut mark = paras[row.end].clone();
        mark.row = Some(Box::new(props));
        new.push(mark);
        self.splice(story, row.first, row.end - row.first + 1, new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::StoryId::Main;
    use crate::text::from_plain_text;

    fn texts(d: &Document) -> Vec<String> {
        d.main
            .paras
            .iter()
            .map(|p| {
                let m = match p.mark {
                    Mark::Paragraph => "",
                    Mark::CellEnd => "|",
                    Mark::RowEnd => "/",
                    Mark::SectionBreak => "=",
                };
                format!("{}{m}", p.text)
            })
            .collect()
    }

    #[test]
    fn inserts_a_table_and_finds_its_rows() {
        let mut d = from_plain_text("Before after");
        let at = d.insert_table(Main, Position::new(0, 7), 2, 3, 2880);
        assert_eq!(at, Position::new(1, 0));
        assert_eq!(
            texts(&d),
            ["Before ", "|", "|", "|", "/", "|", "|", "|", "/", "after"]
        );
        let row = row_at(&d.main.paras, 2).unwrap();
        assert_eq!(row.first, 1);
        assert_eq!(row.end, 4);
        assert_eq!(row.cells, [(1, 1), (2, 2), (3, 3)]);
        assert_eq!(row.cell_of(3), Some(2));
        assert_eq!(table_rows(&d.main.paras, 6).len(), 2);
        assert!(row_at(&d.main.paras, 0).is_none());
        let props = row_props(&d.main.paras, &row, 8640);
        assert_eq!(props.cells.len(), 3);
        assert_eq!(props.cells[0].width, 2880);
        // No tables inside tables.
        assert_eq!(
            d.insert_table(Main, Position::new(2, 0), 1, 1, 100),
            Position::new(2, 0)
        );
    }

    #[test]
    fn converts_tabbed_text_to_a_table() {
        let mut d = from_plain_text("Name\tAmount\nApples\t42\nPears");
        d.convert_to_table(Main, 0, 2, Convert::Tabs, |cols| 8640 / cols as Twips);
        assert_eq!(
            texts(&d),
            [
                "Name|", "Amount|", "/", "Apples|", "42|", "/", "Pears|", "|", "/", ""
            ]
        );
        let row = row_at(&d.main.paras, 0).unwrap();
        assert_eq!(row_props(&d.main.paras, &row, 8640).cells[1].width, 4320);
    }

    #[test]
    fn converts_paragraphs_into_cells() {
        let mut d = from_plain_text("a\nb\nc");
        d.convert_to_table(Main, 0, 2, Convert::Paragraphs(2), |_| 100);
        assert_eq!(texts(&d), ["a|", "b|", "/", "c|", "|", "/", ""]);
    }

    #[test]
    fn rows_and_cells_come_and_go_and_undo() {
        let mut d = from_plain_text("");
        d.insert_table(Main, Position::new(0, 0), 1, 2, 1000);
        d.commit(crate::Selection::default());
        let row = row_at(&d.main.paras, 0).unwrap();
        let props = row_props(&d.main.paras, &row, 8640);
        d.insert_rows(Main, &row, true, 1, &props);
        assert_eq!(texts(&d), ["|", "|", "/", "|", "|", "/", ""]);
        let row = row_at(&d.main.paras, 0).unwrap();
        d.insert_cells(Main, &row, &props, 1, 1, 500);
        assert_eq!(texts(&d), ["|", "|", "|", "/", "|", "|", "/", ""]);
        let row = row_at(&d.main.paras, 0).unwrap();
        let p = row_props(&d.main.paras, &row, 8640);
        assert_eq!(
            p.cells.iter().map(|c| c.width).collect::<Vec<_>>(),
            [1000, 500, 1000]
        );
        d.delete_cells(Main, &row, &p, 0, 0);
        assert_eq!(texts(&d), ["|", "|", "/", "|", "|", "/", ""]);
        d.commit(crate::Selection::default());
        let second = row_at(&d.main.paras, 3).unwrap();
        d.delete_rows(Main, &second, &second);
        assert_eq!(texts(&d), ["|", "|", "/", ""]);
        d.commit(crate::Selection::default());
        d.undo();
        assert_eq!(texts(&d), ["|", "|", "/", "|", "|", "/", ""]);
    }

    #[test]
    fn merging_cells_keeps_their_text() {
        let mut d = from_plain_text("a\tb\tc");
        d.convert_to_table(Main, 0, 0, Convert::Tabs, |_| 1000);
        let row = row_at(&d.main.paras, 0).unwrap();
        let p = row_props(&d.main.paras, &row, 8640);
        d.merge_cells(Main, &row, &p, 0, 1);
        assert_eq!(texts(&d), ["a", "b|", "c|", "/", ""]);
        let row = row_at(&d.main.paras, 0).unwrap();
        assert_eq!(row.cells, [(0, 1), (2, 2)]);
        let p = row_props(&d.main.paras, &row, 8640);
        assert_eq!(p.cells[0].width, 2000);
        assert_eq!(p.cells.len(), 2);
    }
}
