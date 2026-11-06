//! A selection that runs from one cell of a table to another takes whole
//! cells: the rectangle of rows and columns between the two, as in a
//! spreadsheet. Formatting, Delete and Copy then work cell by cell.

use super::*;

/// The selected cells: columns `from..=to` of each of `rows`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellBlock {
    pub rows: Vec<table::Row>,
    pub from: usize,
    pub to: usize,
}

impl CellBlock {
    /// The columns selected in `row`; a row with fewer cells gives what
    /// it has.
    pub fn columns(&self, row: &table::Row) -> std::ops::RangeInclusive<usize> {
        let last = row.cells.len().saturating_sub(1);
        self.from.min(last)..=self.to.min(last)
    }

    /// Whether every cell of the table is selected, so Cut and Delete
    /// take the table itself.
    pub fn whole_table(&self, paras: &[Paragraph]) -> bool {
        let (Some(top), Some(bottom)) = (self.rows.first(), self.rows.last()) else {
            return false;
        };
        let starts = top.first == 0 || paras[top.first - 1].mark != Mark::RowEnd;
        let ends = paras.get(bottom.end + 1).is_none_or(|p| !p.pap.in_table);
        let widest = self.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
        starts && ends && self.from == 0 && self.to + 1 >= widest
    }

    /// Each selected cell's first and last paragraph, row by row.
    pub fn cells(&self) -> Vec<(usize, usize)> {
        self.rows
            .iter()
            .flat_map(|r| self.columns(r).map(move |k| r.cells[k]))
            .collect()
    }
}

impl DocView {
    /// The cells selected, when the selection runs from one cell of a
    /// table to another cell of the same table.
    pub fn cell_block(&self) -> Option<CellBlock> {
        if self.sel.is_empty() {
            return None;
        }
        let (s, e) = self.sel.range();
        self.block_between(s, e)
    }

    /// The cells to show as selected: as `cell_block`, and also every cell
    /// of a table selected whole, whose selection ends just past the table.
    pub fn selected_cells(&self) -> Option<CellBlock> {
        self.cell_block().or_else(|| {
            let (s, e) = self.sel.range();
            let paras = &self.doc.main.paras;
            if s == e || e.offset != 0 || e.para == 0 || paras[e.para - 1].mark != Mark::RowEnd {
                return None;
            }
            let block = self.block_between(s, Position::new(e.para - 1, 0))?;
            block.whole_table(paras).then_some(block)
        })
    }

    /// Whether `pos` is in a selected cell. A block of cells runs past the
    /// range its selection covers, so the range does not say.
    pub(super) fn in_selected_cells(&self, pos: Position) -> bool {
        self.selected_cells().is_some_and(|block| {
            block.rows.iter().any(|row| {
                row.cell_of(pos.para)
                    .is_some_and(|k| block.columns(row).contains(&k))
            })
        })
    }

    fn block_between(&self, s: Position, e: Position) -> Option<CellBlock> {
        let paras = &self.doc.main.paras;
        if !paras[s.para].pap.in_table || !paras[e.para].pap.in_table {
            return None;
        }
        let first = table::row_at(paras, s.para)?;
        // Both ends are in cells of tables this deep: a selection from a
        // table into one in its cell is ordinary text.
        let level = paras[first.end].pap.level();
        if paras[e.para].pap.level() < level {
            return None;
        }
        let mut rows = vec![first];
        while rows.last()?.end < e.para {
            let next = rows.last()?.end + 1;
            // The two ends are in different tables.
            if paras[next - 1].mark != Mark::RowEnd || paras[next].pap.level() < level {
                return None;
            }
            let row = table::row_at_level(paras, next, level)?;
            if paras[row.end].pap.level() != level {
                return None;
            }
            rows.push(row);
        }
        // The last end may be in a table deeper than the rows: only when
        // it is in one of their cells.
        let last_end = rows.last()?.end;
        if table::row_at_level(paras, e.para, level).is_none_or(|r| r.end != last_end) {
            return None;
        }
        let (top, bottom) = (&rows[0], rows.last()?);
        let a = top.cell_of(s.para).unwrap_or(top.cells.len() - 1);
        let b = bottom.cell_of(e.para).unwrap_or(bottom.cells.len() - 1);
        if rows.len() == 1 && a == b {
            return None;
        }
        Some(CellBlock {
            rows,
            from: a.min(b),
            to: a.max(b),
        })
    }

    /// Empty the selected cells, leaving the table as it is. Returns the
    /// start of the first cell.
    pub(super) fn clear_cells(&mut self, block: &CellBlock) -> Position {
        let cells = block.cells();
        for &(a, b) in cells.iter().rev() {
            let len = self.doc.main.paras[b].len();
            if b > a || len > 0 {
                self.doc
                    .delete(MAIN, Position::new(a, 0), Position::new(b, len));
            }
        }
        Position::new(cells.first().map_or(0, |c| c.0), 0)
    }
}

/// What one cell of a paste holds: paragraphs copied inside Quill, or text
/// from another program.
enum Piece {
    Rich(Fragment),
    Text(String),
}

/// The cells a paste brings, row by row. A table copied in Quill or tab and
/// line separated text give one piece for each cell; anything else is one.
fn paste_grid(content: Clip, doc: &mut Document) -> Vec<Vec<Piece>> {
    match content {
        Clip::Rich(rich) => {
            let fragment = rich.prepare(doc);
            if !fragment.paras.first().is_some_and(|p| p.pap.in_table) {
                return vec![vec![Piece::Rich(fragment)]];
            }
            let mut rows: Vec<Vec<Piece>> = vec![Vec::new()];
            let mut cell: Vec<Paragraph> = Vec::new();
            for p in fragment.paras {
                match p.mark {
                    Mark::RowEnd => {
                        cell.clear();
                        rows.push(Vec::new());
                    }
                    Mark::CellEnd => {
                        cell.push(p);
                        let piece = Fragment {
                            paras: std::mem::take(&mut cell),
                            ends_with_mark: false,
                            objects: fragment.objects.clone(),
                            notes: Vec::new(),
                        };
                        rows.last_mut().expect("a row").push(Piece::Rich(piece));
                    }
                    _ => cell.push(p),
                }
            }
            rows.retain(|r| !r.is_empty());
            rows
        }
        Clip::Plain(text) => {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            let text = text.strip_suffix('\n').unwrap_or(&text);
            text.split('\n')
                .map(|line| {
                    line.split('\t')
                        .map(|c| Piece::Text(c.to_string()))
                        .collect()
                })
                .collect()
        }
        Clip::Picture(_) => Vec::new(),
    }
}

impl DocView {
    /// Paste into a block of selected cells the way a spreadsheet does: a
    /// single value fills every selected cell, a block of values goes in
    /// cell for cell from the first selected cell, as far as the table
    /// reaches. Returns the start of the first cell filled.
    pub(super) fn paste_into_cells(&mut self, block: &CellBlock, content: Clip) -> Position {
        let grid = paste_grid(content, &mut self.doc);
        let paras = &self.doc.main.paras;
        let first = block.rows[0].first;
        let table_rows = table::table_rows(paras, first);
        let top = table_rows
            .iter()
            .position(|r| r.first == first)
            .unwrap_or(0);
        let single = grid.len() == 1 && grid[0].len() == 1;
        // Each destination cell and the piece it gets.
        let mut jobs: Vec<((usize, usize), &Piece)> = Vec::new();
        if single {
            for (a, b) in block.cells() {
                jobs.push(((a, b), &grid[0][0]));
            }
        } else {
            for (i, pieces) in grid.iter().enumerate() {
                let Some(row) = table_rows.get(top + i) else {
                    break;
                };
                for (j, piece) in pieces.iter().enumerate() {
                    if let Some(&cell) = row.cells.get(block.from + j) {
                        jobs.push((cell, piece));
                    }
                }
            }
        }
        let start = jobs.first().map_or(first, |j| j.0.0);
        // Last cell first, so the cells still to do keep their places.
        for ((a, b), piece) in jobs.into_iter().rev() {
            let len = self.doc.main.paras[b].len();
            if b > a || len > 0 {
                self.doc
                    .delete(MAIN, Position::new(a, 0), Position::new(b, len));
            }
            let at = Position::new(a, 0);
            match piece {
                Piece::Rich(fragment) => {
                    self.doc.insert_fragment(MAIN, at, fragment);
                }
                Piece::Text(text) if !text.is_empty() => {
                    let chp = self.doc.main.paras[a].chp_at(0);
                    self.doc.insert_plain(MAIN, at, text, chp);
                }
                Piece::Text(_) => {}
            }
        }
        Position::new(start, 0)
    }
}

/// Cell alignment choices, as the right-click menu lists them: up and
/// down, then across.
pub const CELL_ALIGNMENTS: [(&str, quill_core::props::VAlign, Align); 9] = {
    use quill_core::props::VAlign::{Bottom, Center as Middle, Top};
    [
        ("Top Left", Top, Align::Left),
        ("Top Center", Top, Align::Center),
        ("Top Right", Top, Align::Right),
        ("Center Left", Middle, Align::Left),
        ("Center", Middle, Align::Center),
        ("Center Right", Middle, Align::Right),
        ("Bottom Left", Bottom, Align::Left),
        ("Bottom Center", Bottom, Align::Center),
        ("Bottom Right", Bottom, Align::Right),
    ]
};

impl DocView {
    /// The selected cells, or the cell holding the insertion point.
    fn target_cells(&self) -> Option<CellBlock> {
        if let Some(block) = self.cell_block() {
            return Some(block);
        }
        let paras = &self.doc.main.paras;
        let p = self.sel.focus.para;
        let row = table::row_at(paras, p)?;
        let k = row.cell_of(p).unwrap_or(row.cells.len() - 1);
        Some(CellBlock {
            rows: vec![row],
            from: k,
            to: k,
        })
    }

    /// How the text of the cell holding the insertion point lines up.
    pub fn cell_alignment(&self) -> Option<(quill_core::props::VAlign, Align)> {
        let paras = &self.doc.main.paras;
        let p = self.sel.focus.para;
        let row = table::row_at(paras, p)?;
        let k = row.cell_of(p).unwrap_or(row.cells.len() - 1);
        let props = table::row_props(paras, &row, self.column_twips(p));
        Some((props.cells[k].valign, paras[row.cells[k].0].pap.align))
    }

    /// Line up the text of each selected cell: `v` up and down, `h` across.
    pub fn align_cells(&mut self, v: quill_core::props::VAlign, h: Align) {
        let Some(block) = self.target_cells() else {
            return;
        };
        let tw = self.column_twips(self.sel.focus.para);
        self.doc.begin("Cell Alignment", self.sel);
        for row in &block.rows {
            let mut props = table::row_props(&self.doc.main.paras, row, tw);
            for k in block.columns(row) {
                props.cells[k].valign = v;
                let (a, b) = row.cells[k];
                self.doc.format_paras(MAIN, a, b, |p| p.align = h);
            }
            self.doc.set_row(MAIN, row.end, props);
        }
        let sel = self.sel;
        self.finish_edit(sel);
    }
}
