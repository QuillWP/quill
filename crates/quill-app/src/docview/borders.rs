//! Dragging a cell border with the mouse: a vertical border changes the
//! widths of the cells either side of it (the last one grows the table), a
//! horizontal one the height of the row above it.

use super::*;
use quill_core::props::RowProps;

/// How near, in points, the pointer has to be to a border to grab it.
const GRAB: f32 = 3.0;

/// The narrowest a cell can be dragged to, besides its padding: 0.05 in.
const MIN_TEXT: i32 = 72;

/// A border under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderKind {
    /// Between cells, or at a row's left or right edge.
    Column,
    /// Under a row.
    Row,
}

#[derive(Clone, Debug)]
struct BorderHit {
    kind: BorderKind,
    /// The row's end mark.
    end: usize,
    /// Where the boundary is, across.
    x: f32,
    /// The row's height now, in points.
    height: f32,
}

/// A border being dragged. `rows` are the rows it moves in, with their
/// settings when the drag began and the boundary they are moving.
#[derive(Clone, Debug)]
pub struct BorderDrag {
    kind: BorderKind,
    from: (f32, f32),
    rows: Vec<(usize, RowProps, usize)>,
    height: f32,
    started: bool,
}

impl DocView {
    /// The document point under a window point.
    fn doc_point(&self, wx: f32, wy: f32) -> (f32, f32) {
        self.doc_at(
            wx - self.area.x - self.text_x(),
            wy - self.area.y + self.scroll_y,
        )
    }

    /// The row geometry of the row whose first paragraph is `first`.
    fn row_geom(&self, first: usize) -> Option<&RowGeom> {
        let b = &self.blocks[self.block_at(*self.tops.get(first)?)];
        let row = b.row.as_ref()?;
        if b.first == first {
            return Some(row);
        }
        row.nested
            .iter()
            .find(|n| n.first == first)
            .map(|n| &n.geom)
    }

    /// The border at a document point, the innermost table's first.
    fn border_hit(&self, x: f32, y: f32) -> Option<BorderHit> {
        if self.outline || self.blocks.is_empty() {
            return None;
        }
        let from = self.block_at(y - GRAB);
        let mut found: Vec<(u8, usize, &RowGeom, f32, f32)> = Vec::new();
        for b in self.blocks[from..].iter().take_while(|b| b.top - GRAB <= y) {
            let Some(row) = &b.row else {
                continue;
            };
            found.push((1, b.last, row, b.top, b.height));
            for n in &row.nested {
                found.push((n.level, n.last, &n.geom, n.top, n.height));
            }
        }
        found.sort_by_key(|f| std::cmp::Reverse(f.0));
        for &(_, end, geom, top, height) in &found {
            if y < top - GRAB || y > top + height + GRAB {
                continue;
            }
            let Some(&(.., last_x)) = geom.cells.last() else {
                continue;
            };
            let first_x = geom.cells[0].2;
            let edges: Vec<f32> = geom
                .cells
                .iter()
                .map(|c| c.2)
                .chain(std::iter::once(last_x))
                .collect();
            // A boundary between the row's top and bottom.
            if y >= top
                && y <= top + height
                && let Some(edge) = edges.iter().position(|e| (e - x).abs() <= GRAB)
            {
                return Some(BorderHit {
                    kind: BorderKind::Column,
                    end,
                    x: edges[edge],
                    height,
                });
            }
            // The bottom of the row, along its width.
            if (y - (top + height)).abs() <= GRAB && x >= first_x - GRAB && x <= last_x + GRAB {
                return Some(BorderHit {
                    kind: BorderKind::Row,
                    end,
                    x,
                    height,
                });
            }
        }
        None
    }

    /// What kind of border is under a window point, for the pointer.
    pub fn border_cursor(&self, wx: f32, wy: f32) -> Option<BorderKind> {
        if let Some(d) = &self.border_drag {
            return Some(d.kind);
        }
        let (x, y) = self.doc_point(wx, wy);
        self.border_hit(x, y).map(|h| h.kind)
    }

    /// A press on a border: start dragging it. Returns true if it was one.
    pub(super) fn border_mouse_down(&mut self, wx: f32, wy: f32) -> bool {
        let (x, y) = self.doc_point(wx, wy);
        let Some(hit) = self.border_hit(x, y) else {
            return false;
        };
        let paras = &self.doc.main.paras;
        let Some(row) = table::row_at(paras, hit.end) else {
            return false;
        };
        let text_width = self.column_twips(row.first);
        let mut rows = Vec::new();
        match hit.kind {
            BorderKind::Row => {
                rows.push((hit.end, table::row_props(paras, &row, text_width), 0));
            }
            BorderKind::Column => {
                // Every row of the table with a boundary there moves it.
                for r in table::rows_around(paras, &row) {
                    let Some(geom) = self.row_geom(r.first) else {
                        continue;
                    };
                    let mut edges: Vec<f32> = geom.cells.iter().map(|c| c.2).collect();
                    edges.extend(geom.cells.last().map(|c| c.3));
                    if let Some(edge) = edges.iter().position(|e| (e - hit.x).abs() <= 1.0) {
                        rows.push((r.end, table::row_props(paras, &r, text_width), edge));
                    }
                }
            }
        }
        self.border_drag = Some(BorderDrag {
            kind: hit.kind,
            from: (x, y),
            rows,
            height: hit.height,
            started: false,
        });
        self.drag = None;
        true
    }

    pub(super) fn border_mouse_drag(&mut self, wx: f32, wy: f32) -> bool {
        let Some(d) = self.border_drag.clone() else {
            return false;
        };
        let (x, y) = self.doc_point(wx, wy);
        let (dx, dy) = (x - d.from.0, y - d.from.1);
        if !d.started {
            if dx.abs() < 2.0 && dy.abs() < 2.0 {
                return true;
            }
            let label = match d.kind {
                BorderKind::Column => "Column Width",
                BorderKind::Row => "Row Height",
            };
            self.doc.begin(label, self.sel);
            if let Some(d) = &mut self.border_drag {
                d.started = true;
            }
        }
        match d.kind {
            BorderKind::Column => {
                let dx = (dx * 20.0).round() as i32;
                // However far, no cell goes under its padding and a little.
                let (mut lo, mut hi) = (i32::MIN, i32::MAX);
                for (_, p, edge) in &d.rows {
                    let min = p.gap + MIN_TEXT;
                    let n = p.cells.len();
                    if *edge > 0 {
                        lo = lo.max(min - p.cells[edge - 1].width);
                    }
                    if *edge > 0 && *edge < n {
                        hi = hi.min(p.cells[*edge].width - min);
                    }
                }
                let dx = if lo > hi { 0 } else { dx.clamp(lo, hi) };
                for (end, start, edge) in &d.rows {
                    let mut p = start.clone();
                    let n = p.cells.len();
                    if *edge == 0 {
                        p.indent = start.indent + dx;
                    } else {
                        p.cells[edge - 1].width = start.cells[edge - 1].width + dx;
                        if *edge < n {
                            p.cells[*edge].width = start.cells[*edge].width - dx;
                        }
                    }
                    self.doc.set_row(MAIN, *end, p);
                }
            }
            BorderKind::Row => {
                let twips = (((d.height + dy) * 20.0).round() as i32).max(MIN_TEXT);
                for (end, start, _) in &d.rows {
                    let mut p = start.clone();
                    p.height = match start.height {
                        quill_core::RowHeight::Exactly(_) => quill_core::RowHeight::Exactly(twips),
                        _ => quill_core::RowHeight::AtLeast(twips),
                    };
                    self.doc.set_row(MAIN, *end, p);
                }
            }
        }
        true
    }

    /// Let go of the border: the change so far is one step to undo.
    pub(super) fn border_mouse_up(&mut self) -> bool {
        let Some(d) = self.border_drag.take() else {
            return false;
        };
        if d.started {
            let sel = self.sel;
            self.finish_edit(sel);
            // Dragging the last border out can take the table past its
            // room: ask, as Insert Table does, and put it back if not.
            if d.kind == BorderKind::Column
                && let Some((end, _, _)) = d.rows.first()
            {
                let first = table::row_at(&self.doc.main.paras, *end).map_or(*end, |r| r.first);
                let before = d.rows.iter().map(|(_, p, _)| p.width()).max().unwrap_or(0);
                let after = crate::table_dialogs::widest_after(self, first, &[]);
                if !crate::table_dialogs::confirm_wider(self, first, before, after, "Keep") {
                    if let Some(sel) = self.doc.undo() {
                        self.sel = sel;
                    }
                    self.invalidate_layout();
                }
            }
        }
        true
    }
}
