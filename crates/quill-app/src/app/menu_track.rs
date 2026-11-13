//! Tracking an open menu: where it came from and what it holds.

use super::*;

/// Where an open menu came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum MenuSource {
    Bar(usize),
    Popup(Hit),
    Context,
    Zoom,
}

pub(super) struct MenuTrack {
    pub(super) source: MenuSource,
    pub(super) menus: Vec<Menu>,
    pub(super) actions: Vec<Action>,
    pub(super) open: OpenMenu,
    /// The parent item and open box for one nested menu.
    pub(super) submenu: Option<(usize, OpenMenu)>,
    /// What has been typed to jump to an item, and when.
    pub(super) typed: (String, Option<Instant>),
    pub(super) opened: Instant,
    /// The button is still down from the press that opened it.
    pub(super) held: bool,
}

impl MenuTrack {
    /// Which of `menus` is open: the bar's own index, or the only one.
    pub(super) fn index(&self) -> usize {
        match self.source {
            MenuSource::Bar(i) => i,
            MenuSource::Popup(_) | MenuSource::Context | MenuSource::Zoom => 0,
        }
    }

    pub(super) fn menu(&self) -> &Menu {
        &self.menus[self.index()]
    }
}

impl WinState {
    /// A pop-up menu Quill has just opened (Zoom, the toolbar's own menu,
    /// the formatting toolbar's Font, Size and Style), taken away so the
    /// system can show it instead: the menu, its actions and its top-left
    /// corner in window points.
    #[cfg(target_os = "macos")]
    pub(super) fn take_popup_menu(&mut self) -> Option<(Menu, Vec<Action>, (f64, f64))> {
        if matches!(self.menu.as_ref()?.source, MenuSource::Bar(_)) {
            return None;
        }
        let track = self.menu.take()?;
        let at = (
            (track.open.rect.x * self.ui) as f64,
            (track.open.rect.y * self.ui) as f64,
        );
        let menu = track.menu().clone();
        Some((menu, track.actions, at))
    }
}
