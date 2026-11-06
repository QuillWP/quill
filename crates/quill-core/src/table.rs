//! Tables the original way: no separate table object, just paragraphs
//! marked as being in a table. A cell's last paragraph ends with a cell
//! mark; after a row's cells comes an empty paragraph with the row's end
//! mark, which carries the row's cell widths and settings. Every edit here
//! goes through `splice`, so tables undo like everything else.

use crate::document::{Document, StoryId};
use crate::limits::MAX_TABLE_LEVEL;
use crate::para::{Mark, Paragraph, chars};
use crate::props::{RowProps, Twips};
use crate::story::Position;

/// One row: its paragraphs from `first` to its end mark at `end`, and each
/// cell's first and last paragraph.
/// The narrowest a cell can be and still be split in two: 0.1 in each.
const MIN_SPLIT_WIDTH: Twips = 144;

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

/// Make pasted paragraphs fit where they go, which is `inside` tables deep
/// (0 for ordinary text). Whole rows stay rows: at the top they stay as
/// they are, inside a cell they become a table in it, as long as that
/// does not go past `MAX_TABLE_LEVEL`. Cells copied without their whole
/// row become plain paragraphs, one for each cell.
pub(crate) fn fit_pasted(paras: &mut Vec<Paragraph>, inside: u8) {
    let flatten = |p: &mut Paragraph| {
        p.pap.set_level(inside);
        p.mark = Mark::Paragraph;
        p.row = None;
    };
    let mut i = 0;
    while i < paras.len() {
        if paras[i].pap.level() == 0 {
            i += 1;
            continue;
        }
        let run_end = (i..paras.len())
            .find(|&j| paras[j].pap.level() == 0)
            .unwrap_or(paras.len());
        // Whole rows only: each ends with its row mark, after as many
        // cells as the mark describes, at every depth.
        let mut whole = paras[run_end - 1].mark == Mark::RowEnd;
        let mut cells = [0usize; MAX_TABLE_LEVEL as usize + 1];
        let mut deepest = 0;
        for p in &paras[i..run_end] {
            let level = (p.pap.level() as usize).min(cells.len() - 1);
            deepest = deepest.max(level);
            match p.mark {
                Mark::CellEnd => cells[level] += 1,
                Mark::RowEnd => {
                    whole &= p
                        .row
                        .as_ref()
                        .is_some_and(|r| r.cells.len() == cells[level]);
                    cells[level] = 0;
                }
                _ => {}
            }
        }
        if whole && deepest + inside as usize <= MAX_TABLE_LEVEL as usize {
            for p in &mut paras[i..run_end] {
                let level = p.pap.level();
                p.pap.set_level(level + inside);
            }
            i = run_end;
            continue;
        }
        let mut k = i;
        let mut end = run_end;
        while k < end {
            if paras[k].mark == Mark::RowEnd && paras[k].text.is_empty() {
                paras.remove(k);
                end -= 1;
            } else {
                flatten(&mut paras[k]);
                k += 1;
            }
        }
        i = end;
    }
}

/// Make the tables of a story well formed, as files from other programs
/// (and damaged ones) may not be: a table inside a cell has to be closed
/// by its own row marks within a cell of the table round it, and a cell has
/// to end in a paragraph of its own. Paragraphs that cannot be put right
/// become ordinary paragraphs of the cell (or story) they are in, keeping
/// their text.
pub fn repair(paras: &mut Vec<Paragraph>) {
    let end = paras.len();
    repair_range(paras, 0, end, 0);
}

fn repair_range(paras: &mut Vec<Paragraph>, a: usize, mut end: usize, level: u8) {
    let mut j = a;
    // Each pass either moves on or fixes something, but never forever.
    let mut budget = 4 * (end - a) + 16;
    while j < end && budget > 0 {
        budget -= 1;
        if paras[j].pap.level() <= level {
            j += 1;
            continue;
        }
        let row = row_at_level(paras, j, level + 1).filter(|r| r.first == j && r.end < end);
        let Some(row) = row else {
            // Not a row that closes: plain text of this level.
            let p = &mut paras[j];
            p.pap.set_level(level);
            if matches!(p.mark, Mark::CellEnd | Mark::RowEnd) {
                p.mark = Mark::Paragraph;
                p.row = None;
            }
            j += 1;
            continue;
        };
        // A cell that ends with a table of its own (so without a cell mark
        // of its own) gets a paragraph after it.
        let cell_level = level + 1;
        if let Some(&(_, cb)) = row
            .cells
            .iter()
            .find(|&&(_, cb)| paras[cb].pap.level() > cell_level)
        {
            let mut pap = paras[cb].pap.clone();
            pap.set_level(cell_level);
            let mut fix = Paragraph::new(pap, paras[cb].mark_chp);
            fix.mark = Mark::CellEnd;
            paras.insert(cb + 1, fix);
            end += 1;
            continue;
        }
        // Tables in the cells, last cell first so the places hold.
        for &(ca, cb) in row.cells.iter().rev() {
            let before = paras.len();
            repair_range(paras, ca, cb + 1, cell_level);
            end += paras.len() - before;
        }
        let Some(row) = row_at_level(paras, j, level + 1) else {
            j += 1;
            continue;
        };
        j = row.end + 1;
    }
}

/// The row holding paragraph `p`, if it is in a table: the innermost
/// one when the paragraph is in a table inside a cell.
pub fn row_at(paras: &[Paragraph], p: usize) -> Option<Row> {
    let level = paras.get(p)?.pap.level();
    if level == 0 {
        return None;
    }
    row_at_level(paras, p, level)
}

/// The row at table depth `level` holding paragraph `p`, which is that
/// deep or deeper: with `level` 1, the outer table's row round a nested
/// table's paragraph. A row's cells run to its cell marks at its own depth,
/// so a nested table's marks, being deeper, belong to a cell's contents.
pub fn row_at_level(paras: &[Paragraph], p: usize, level: u8) -> Option<Row> {
    if level == 0 || paras.get(p)?.pap.level() < level {
        return None;
    }
    let ends_row = |q: &Paragraph| q.mark == Mark::RowEnd && q.pap.level() == level;
    let mut first = p;
    while first > 0 && paras[first - 1].pap.level() >= level && !ends_row(&paras[first - 1]) {
        first -= 1;
    }
    let mut end = p;
    while end < paras.len() && !ends_row(&paras[end]) {
        if paras[end].pap.level() < level {
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
        if para.mark == Mark::CellEnd && para.pap.level() == level {
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

/// Every row of the table holding paragraph `p`, top to bottom: that of
/// the innermost table `p` is in.
pub fn table_rows(paras: &[Paragraph], p: usize) -> Vec<Row> {
    let Some(row) = row_at(paras, p) else {
        return Vec::new();
    };
    rows_around(paras, &row)
}

/// The rows of the table `row` belongs to, top to bottom.
pub fn rows_around(paras: &[Paragraph], row: &Row) -> Vec<Row> {
    let level = paras[row.end].pap.level();
    let mut first = row.first;
    while first > 0
        && paras[first - 1].mark == Mark::RowEnd
        && paras[first - 1].pap.level() == level
    {
        match row_at_level(paras, first - 1, level) {
            Some(r) => first = r.first,
            None => break,
        }
    }
    let mut rows = Vec::new();
    let mut i = first;
    while let Some(r) = row_at_level(paras, i, level) {
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

/// Whether every row of a table runs margin to margin in `text_width`,
/// allowing for columns that didn't divide evenly.
pub fn is_full_width(paras: &[Paragraph], rows: &[Row], text_width: Twips) -> bool {
    rows.iter().all(|r| {
        let p = row_props(paras, r, text_width);
        (p.width() - text_width).abs() <= (p.cells.len() as Twips).max(15)
    })
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
    /// splitting the paragraph there. Returns the first cell's position.
    /// Inside a cell it makes a table in the cell, up to `MAX_TABLE_LEVEL`
    /// deep; past that, `at` comes back unchanged.
    pub fn insert_table(
        &mut self,
        story: StoryId,
        at: Position,
        rows: usize,
        cols: usize,
        width: Twips,
    ) -> Position {
        let level = self.story(story).paras[at.para].pap.level() + 1;
        if level > MAX_TABLE_LEVEL || rows == 0 || cols == 0 {
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
        for q in &mut new {
            q.pap.set_level(level);
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
    /// Keep tables that ran margin to margin doing so when the margins or
    /// the paper change from `was` to `now` wide. Tables given a width of
    /// their own stay as they are.
    pub(crate) fn fit_full_width_tables(&mut self, was: Twips, now: Twips) {
        if was == now || was <= 0 || now <= 0 {
            return;
        }
        let story = StoryId::Main;
        let mut p = 0;
        while p < self.main.paras.len() {
            if !self.main.paras[p].pap.in_table {
                p += 1;
                continue;
            }
            let rows = table_rows(&self.main.paras, p);
            let Some(last) = rows.last() else {
                p += 1;
                continue;
            };
            p = last.end + 1;
            if is_full_width(&self.main.paras, &rows, was) {
                self.fit_rows(story, &rows, was, now);
            }
        }
    }

    /// Stretch or shrink each of `rows` so it is `now` wide, keeping the
    /// columns in proportion. The last cell takes what rounding leaves.
    pub fn fit_rows(&mut self, story: StoryId, rows: &[Row], text_width: Twips, now: Twips) {
        for row in rows {
            let mut r = row_props(&self.story(story).paras, row, text_width);
            let old = r.width().max(1) as i64;
            let mut left = now;
            let n = r.cells.len();
            for (k, c) in r.cells.iter_mut().enumerate() {
                c.width = if k + 1 == n {
                    left
                } else {
                    (c.width as i64 * now as i64 / old) as Twips
                };
                left -= c.width;
            }
            self.set_row(story, row.end, r);
        }
    }

    /// Copy columns `from..=to` of `rows` as a smaller table of whole rows,
    /// each row mark keeping just those cells' settings.
    pub fn copy_cells(
        &self,
        story: StoryId,
        rows: &[Row],
        from: usize,
        to: usize,
        text_width: Twips,
    ) -> crate::document::Fragment {
        let paras = &self.story(story).paras;
        let mut out = Vec::new();
        for row in rows {
            let last = row.cells.len().saturating_sub(1);
            let cols = from.min(last)..=to.min(last);
            for k in cols.clone() {
                let (a, b) = row.cells[k];
                out.extend(paras[a..=b].iter().cloned());
            }
            let mut props = row_props(paras, row, text_width);
            props.cells = props.cells[cols].to_vec();
            let mut mark = paras[row.end].clone();
            mark.row = Some(Box::new(props));
            out.push(mark);
        }
        self.with_objects(crate::document::Fragment {
            paras: out,
            ends_with_mark: true,
            objects: Vec::new(),
            notes: Vec::new(),
        })
    }

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

    /// Delete `s..e` when it holds whole tables: they go entirely, with the
    /// text around them. None, changing nothing, when `s` or `e` is partway
    /// into a table, or while tracking changes (a deleted table cannot be
    /// tracked yet, so only its text is marked deleted).
    pub fn delete_whole_tables(
        &mut self,
        story: StoryId,
        s: Position,
        e: Position,
    ) -> Option<Position> {
        let paras = &self.story(story).paras;
        let table_start = |p: usize| p == 0 || !paras[p - 1].pap.in_table;
        let start_ok = !paras[s.para].pap.in_table || (s.offset == 0 && table_start(s.para));
        if self.props.track_changes || !start_ok || paras[e.para].pap.in_table {
            return None;
        }
        // Table paragraphs in the range, as runs, last first.
        let mut runs = Vec::new();
        let mut p = s.para;
        while p < e.para {
            if paras[p].pap.in_table {
                let a = p;
                while p < e.para && paras[p].pap.in_table {
                    p += 1;
                }
                runs.push((a, p - a));
            } else {
                p += 1;
            }
        }
        if runs.is_empty() {
            return None;
        }
        let mut e = e;
        for &(a, n) in runs.iter().rev() {
            self.splice(story, a, n, Vec::new());
            e.para -= n;
        }
        // A range starting at a table now starts where the table was.
        let s = if s.para > e.para { e } else { s };
        Some(if s < e { self.delete(story, s, e) } else { s })
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

    /// Format > Table Layout > Split Cell: each of cells `from..=to` of a row
    /// becomes two side by side, sharing its width and borders. The text
    /// stays in the first. Cells too narrow to halve are left alone.
    pub fn split_cells(
        &mut self,
        story: StoryId,
        row: &Row,
        props: &RowProps,
        from: usize,
        to: usize,
    ) {
        let to = to.min(row.cells.len() - 1).min(props.cells.len() - 1);
        if from > to {
            return;
        }
        let mut new_props = props.clone();
        new_props.cells.clear();
        let mut new: Vec<Paragraph> = Vec::new();
        for (k, &(a, b)) in row.cells.iter().enumerate() {
            let paras = &self.story(story).paras;
            new.extend_from_slice(&paras[a..=b]);
            let cell = &props.cells[k.min(props.cells.len() - 1)];
            if k >= from && k <= to && cell.width >= 2 * MIN_SPLIT_WIDTH {
                let first = cell.width - cell.width / 2;
                let second = cell.width / 2;
                new_props.cells.push(crate::props::CellProps {
                    width: first,
                    ..cell.clone()
                });
                new_props.cells.push(crate::props::CellProps {
                    width: second,
                    ..cell.clone()
                });
                new.push(self.empty_cell(story, a));
            } else {
                new_props.cells.push(cell.clone());
            }
        }
        let mut mark = self.story(story).paras[row.end].clone();
        mark.row = Some(Box::new(new_props));
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

    #[test]
    fn full_width_tables_follow_the_margins() {
        let mut d = from_plain_text("a\nb\nc");
        let tw = d.props.text_width();
        d.insert_table(Main, Position::new(0, 0), 2, 3, tw / 3);
        d.insert_table(Main, Position::new(9, 0), 1, 2, 1440);
        let widths = |d: &Document| -> Vec<Twips> {
            d.main
                .paras
                .iter()
                .filter_map(|p| p.row.as_ref().map(|r| r.width()))
                .collect()
        };
        assert_eq!(widths(&d), [tw / 3 * 3, tw / 3 * 3, 2880]);
        let mut props = d.props.clone();
        props.margin_left += 720;
        props.margin_right += 720;
        let sel = crate::Selection::caret(Position::new(0, 0));
        assert!(d.set_document_props("Document Setup", props, sel));
        // The full-width table fills the narrower text; the one with a
        // width of its own keeps it.
        let now = d.props.text_width();
        assert_eq!(widths(&d), [now, now, 2880]);
        d.undo();
        assert_eq!(d.props.text_width(), tw);
        assert_eq!(widths(&d), [tw / 3 * 3, tw / 3 * 3, 2880]);
    }

    #[test]
    fn a_table_in_a_cell_is_found_as_its_own_rows_and_the_outer_row_goes_round_it() {
        let mut d = Document::new();
        d.insert_table(Main, Position::new(0, 0), 1, 2, 3000);
        // Into the first cell: a table of two rows by two cells.
        let at = d.insert_table(Main, Position::new(0, 0), 2, 2, 1000);
        assert_eq!(at, Position::new(0, 0));
        let paras = &d.main.paras;
        // Inner rows first, then the outer cell's own paragraph, the
        // second outer cell and the outer row mark.
        let levels: Vec<u8> = paras.iter().map(|p| p.pap.level()).collect();
        assert_eq!(levels, [2, 2, 2, 2, 2, 2, 1, 1, 1, 0]);
        let inner = table_rows(paras, 0);
        assert_eq!(inner.len(), 2);
        assert!(inner.iter().all(|r| r.cells.len() == 2));
        let outer = row_at_level(paras, 0, 1).unwrap();
        assert_eq!(outer.cells, [(0, 6), (7, 7)]);
        assert_eq!(row_at(paras, 6).unwrap(), outer);
        assert_eq!(table_rows(paras, 7).len(), 1);
        // The inner table inside a nest of its own.
        assert_eq!(row_at_level(paras, 3, 2).unwrap(), inner[1]);
        assert!(row_at_level(paras, 6, 2).is_none());
    }

    #[test]
    fn whole_tables_paste_into_a_cell_as_nested_tables_and_pieces_as_text() {
        let mut d = Document::new();
        d.insert_table(Main, Position::new(0, 0), 1, 1, 3000);
        let mut src = Document::new();
        src.insert_table(Main, Position::new(0, 0), 1, 2, 1000);
        let last = src.main.paras.len() - 1;
        let whole = src.copy(Main, Position::new(0, 0), Position::new(last, 0));
        let end = d.insert_fragment(Main, Position::new(0, 0), &whole);
        let levels: Vec<u8> = d.main.paras.iter().map(|p| p.pap.level()).collect();
        assert_eq!(levels, [2, 2, 2, 1, 1, 0]);
        assert_eq!(end, Position::new(3, 0));
        // Cells without their row become text in the cell.
        let mut e = Document::new();
        e.insert_table(Main, Position::new(0, 0), 1, 1, 3000);
        let piece = src.copy(Main, Position::new(0, 0), Position::new(0, 0));
        e.insert_fragment(Main, Position::new(0, 0), &piece);
        assert!(e.main.paras.iter().all(|p| p.pap.level() <= 1));
    }

    #[test]
    fn repair_puts_nested_tables_right_or_makes_them_text() {
        use crate::props::ParaProps;
        let para = |level: u8, mark: Mark, text: &str| {
            let mut pap = ParaProps::default();
            pap.set_level(level);
            let mut p = Paragraph::new(pap, Default::default());
            p.mark = mark;
            p.insert(0, text, Default::default());
            p
        };
        let levels = |v: &[Paragraph]| -> Vec<(u8, Mark)> {
            v.iter().map(|p| (p.pap.level(), p.mark)).collect()
        };
        use Mark::{CellEnd, RowEnd};
        // A table in a table that never closes: its text stays, as text.
        let mut v = vec![
            para(1, CellEnd, "a"),
            para(1, RowEnd, ""),
            para(2, CellEnd, "x"),
            para(2, RowEnd, ""),
        ];
        repair(&mut v);
        assert_eq!(
            levels(&v),
            [
                (1, CellEnd),
                (1, RowEnd),
                (0, Mark::Paragraph),
                (0, Mark::Paragraph)
            ]
        );
        assert_eq!(v[2].text, "x");
        // A cell that ends with a table in it gets a paragraph to end on.
        let mut v = vec![
            para(2, CellEnd, "x"),
            para(2, RowEnd, ""),
            para(1, RowEnd, ""),
        ];
        repair(&mut v);
        assert_eq!(
            levels(&v),
            [(2, CellEnd), (2, RowEnd), (1, CellEnd), (1, RowEnd)]
        );
        // A table that is fine is left as it is.
        let ok = vec![
            para(1, Mark::Paragraph, "before"),
            para(2, CellEnd, "a"),
            para(2, RowEnd, ""),
            para(1, CellEnd, "after"),
            para(1, RowEnd, ""),
            para(0, Mark::Paragraph, "end"),
        ];
        let mut v = ok.clone();
        repair(&mut v);
        assert_eq!(levels(&v), levels(&ok));
    }
}
