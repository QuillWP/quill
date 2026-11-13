//! Tooltips for the controls the pointer rests on.

use super::*;

/// How long the pointer rests on a control before its tooltip shows.
pub(crate) const TIP_DELAY: Duration = Duration::from_millis(600);

/// A tooltip waiting to show, or showing.
pub(super) struct Tip {
    pub(super) text: String,
    /// The pointer when it reached the control, in interface units.
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) due: Instant,
    pub(super) shown: bool,
}

impl WinState {
    /// The pointer reached a different control (or none): start that
    /// control's tooltip. Moving from one showing tip to the next control
    /// shows the next at once, as system tooltips do.
    pub(super) fn retip(&mut self, x: f32, y: f32) {
        let text = self
            .revision_hover
            .and_then(|i| self.revision_badges.get(i).map(|b| b.tip.clone()))
            .or_else(|| match (self.toolbar_hover, self.bar_hover) {
                (Some(i), _) => toolbar::tip(i),
                (None, Some(hit)) => bars::tip(hit),
                (None, None) => None,
            });
        let quick = self.tip.as_ref().is_some_and(|t| t.shown);
        let now = Instant::now();
        self.tip = text.map(|text| Tip {
            text,
            x,
            y,
            due: if quick { now } else { now + TIP_DELAY },
            shown: quick,
        });
    }
}
