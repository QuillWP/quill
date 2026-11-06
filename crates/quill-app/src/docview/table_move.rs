//! The table move handle: a small box at a table's top left corner while
//! the insertion point is in it. Click it to select the whole table; drag
//! it to another paragraph to move the table there.

use super::*;

/// The handle is this many points square, just outside the table.
const SIZE: f32 = 11.0;

/// The handle being dragged.
#[derive(Clone, Copy, Debug)]
pub struct TableDrag {
    /// The table's first and last paragraphs.
    pub first: usize,
    pub last: usize,
    /// Where the pointer is now, and whether it has moved off the handle.
    pub to: (f32, f32),
    pub moved: bool,
}

impl DocView {
    /// The table the insertion point is in, and its handle in window
    /// points.
    pub(super) fn table_handle(&self) -> Option<(usize, usize, Rect)> {
        if self.outline {
            return None;
        }
        let (first, last) = self.table_around(self.sel.start().para)?;
        let b = self.blocks.iter().find(|b| b.first == first)?;
        let left = b.row.as_ref()?.cells.first()?.2;
        let (x, y) = self.to_window(left, b.top);
        // In Normal view the table can start at the window's edge.
        let x = (x - SIZE - 2.0).max(self.area.x + 1.0);
        Some((first, last, Rect::new(x, y - SIZE - 2.0, SIZE, SIZE)))
    }

    /// Where a dragged table would go: before paragraph `p`, which is not
    /// in a table nor inside the table being moved. None to stay put.
    fn table_drop(&self, d: &TableDrag) -> Option<usize> {
        let (pos, _) = self.hit(d.to.0, d.to.1);
        let mut p = pos.para;
        // Below a paragraph's middle line, the drop goes after it.
        if let Some(l) = self.layouts.get(p).and_then(|l| l.as_ref())
            && let (_, wy) = self.to_window(0.0, self.tops[p] + l.height / 2.0)
            && d.to.1 > wy
        {
            p += 1;
        }
        let paras = &self.doc.main.paras;
        let ok = p < paras.len()
            && !paras[p].pap.in_table
            && (p == 0 || !paras[p - 1].pap.in_table || p == d.last + 1)
            && !(d.first..=d.last + 1).contains(&p);
        ok.then_some(p)
    }

    /// The table whose handle is under a window point, allowing a little
    /// slack around the box.
    fn table_handle_at(&self, wx: f32, wy: f32) -> Option<(usize, usize)> {
        let (first, last, r) = self.table_handle()?;
        let grow = Rect::new(r.x - 2.0, r.y - 2.0, r.w + 4.0, r.h + 4.0);
        grow.contains(wx, wy).then_some((first, last))
    }

    /// Whether a window point is on the handle. A table at the top of a
    /// page has its handle in the header area, and the handle comes first.
    pub(super) fn on_table_handle(&self, wx: f32, wy: f32) -> bool {
        self.table_handle_at(wx, wy).is_some()
    }

    /// A click on the handle: select the table and start a move. Returns
    /// true if the click was the handle's.
    pub(super) fn table_mouse_down(&mut self, wx: f32, wy: f32) -> bool {
        let Some((first, last)) = self.table_handle_at(wx, wy) else {
            return false;
        };
        self.select_table();
        self.drag = None;
        self.table_drag = Some(TableDrag {
            first,
            last,
            to: (wx, wy),
            moved: false,
        });
        true
    }

    pub(super) fn table_mouse_drag(&mut self, wx: f32, wy: f32) -> bool {
        let Some(d) = &mut self.table_drag else {
            return false;
        };
        d.moved |= (wx - d.to.0).abs() > 3.0 || (wy - d.to.1).abs() > 3.0;
        d.to = (wx, wy);
        true
    }

    /// Let go of the handle: move the table to where it was dropped, as
    /// one step to undo.
    pub(super) fn table_mouse_up(&mut self) -> bool {
        let Some(d) = self.table_drag.take() else {
            return false;
        };
        let Some(to) = d.moved.then(|| self.table_drop(&d)).flatten() else {
            return true;
        };
        if self.doc.props.track_changes {
            self.status = Some("A table can't be moved while Track Changes is on.".into());
            return true;
        }
        let start = Position::new(d.first, 0);
        let end = Position::new(d.last + 1, 0);
        let table = self.doc.copy(MAIN, start, end);
        self.doc.begin("Move Table", self.sel);
        self.doc.delete_whole_tables(MAIN, start, end);
        let count = d.last + 1 - d.first;
        let to = if to > d.last { to - count } else { to };
        let after = self.doc.insert_fragment(MAIN, Position::new(to, 0), &table);
        self.finish_edit(Selection::new(Position::new(to, 0), after));
        true
    }

    /// The handle, and while it is dragged, a line where the table would go.
    pub(super) fn draw_table_handle(&self, c: &mut Canvas) {
        let Some((_, _, r)) = self.table_handle() else {
            return;
        };
        c.fill(r, color::WHITE);
        c.frame(r, color::GRAY_DARK);
        // A four-way arrow, drawn as a cross with arrowheads.
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let arm = r.w / 2.0 - 2.0;
        let t = c.hair();
        c.fill(
            Rect::new(cx - arm, cy - t / 2.0, arm * 2.0, t),
            color::BLACK,
        );
        c.fill(
            Rect::new(cx - t / 2.0, cy - arm, t, arm * 2.0),
            color::BLACK,
        );
        for k in [1.0, 2.0] {
            let w = (3.0 - k) * 2.0;
            c.fill(
                Rect::new(cx - w / 2.0, cy - arm + k - 1.0, w, t),
                color::BLACK,
            );
            c.fill(Rect::new(cx - w / 2.0, cy + arm - k, w, t), color::BLACK);
            c.fill(
                Rect::new(cx - arm + k - 1.0, cy - w / 2.0, t, w),
                color::BLACK,
            );
            c.fill(Rect::new(cx + arm - k, cy - w / 2.0, t, w), color::BLACK);
        }
        if let Some(d) = &self.table_drag
            && d.moved
            && let Some(p) = self.table_drop(d)
        {
            let (x, y) = self.to_window(self.xs[p], self.tops[p]);
            let w = (self.column_twips(p) as f32 / 20.0).max(40.0);
            c.fill(Rect::new(x, y - 1.0, w, 2.0), color::BLACK);
        }
    }
}
