//! The formatting toolbar's Font Color: the same palette that pops out
//! beside a dialog's color box, opened below the toolbar button. One click
//! on a box colors the selection and puts the palette away.

use super::*;
use crate::dialog::Popout;
use crate::palette::{self, PaletteState, Pick, Target};

pub(super) struct ToolPalette {
    /// Where the panel is, in interface units.
    pub(super) panel: Rect,
    pub(super) pop: Popout,
    st: PaletteState,
}

impl WinState {
    /// Open the palette below `anchor` (the toolbar button), marking the selection's color.
    pub(super) fn open_tool_palette(&mut self, anchor: Rect) {
        let start = Some(self.view.current_chp().color);
        let st = PaletteState::new(start, &self.view.doc);
        let pop = palette::popout(&st, "", false);
        let (w, h) = pop.size;
        let x = anchor.x.min(self.width - w - 8.0).max(8.0);
        self.tool_pal = Some(ToolPalette {
            panel: Rect::new(x, anchor.bottom() + 2.0, w, h),
            pop,
            st,
        });
    }

    pub(super) fn draw_tool_palette(&self, c: &mut Canvas, label: &mut Label) {
        if let Some(t) = &self.tool_pal {
            crate::dialog::draw_popout_panel(c, label, t.panel, &t.pop);
        }
    }

    /// A press while the palette is open: a box colors the text, Edit
    /// Color… opens the editor, and a press anywhere else puts it away.
    /// None when the palette is not open.
    pub(super) fn tool_palette_down(&mut self, x: f32, y: f32) -> Option<Effect> {
        let t = self.tool_pal.as_mut()?;
        if !t.panel.contains(x, y) {
            self.tool_pal = None;
            return Some(Effect::None);
        }
        let (lx, ly) = (x - t.panel.x, y - t.panel.y);
        let hit = t
            .pop
            .items
            .iter()
            .find(|it| it.enabled && it.rect.contains(lx, ly))
            .map(|it| (it.id, it.label.is_empty()));
        match hit {
            Some((_, false)) => {
                let (start, _) = palette::edit_of(&t.st, None);
                self.tool_pal = None;
                self.edit_color(start, Target::Text);
            }
            Some((id, true)) => {
                if let Some(Pick::Chose(color)) = palette::pop_swatch(&mut t.st, id, false) {
                    self.tool_pal = None;
                    self.view.set_color(color);
                }
            }
            None => {}
        }
        Some(Effect::None)
    }
}
