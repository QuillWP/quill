//! Editable toolbar zoom and one-shot fitting of the visible page.
use super::*;
use crate::canvas::round_rect;

const WIDTH: f32 = 112.0;
const PRESETS: [f64; 5] = [50.0, 75.0, 100.0, 125.0, 150.0];
pub(super) const MIN: f64 = 10.0;
pub(super) const MAX: f64 = 800.0;

pub(super) struct Edit {
    text: String,
    anchor: usize,
    caret: usize,
}

fn percent_text(percent: f64) -> String {
    format!("{percent:.2}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
        + "%"
}

fn parse(text: &str) -> Option<f64> {
    let text = text.trim();
    let n: f64 = text.strip_suffix('%').unwrap_or(text).trim().parse().ok()?;
    (n.is_finite() && (MIN..=MAX).contains(&n)).then_some((n * 100.0).round() / 100.0)
}

impl Edit {
    fn replace(&mut self, text: &str) {
        let (a, b) = (self.anchor.min(self.caret), self.anchor.max(self.caret));
        let value: String = text
            .chars()
            .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '%' | ' '))
            .collect();
        if self.text.len() - (b - a) + value.len() > 12 {
            return;
        }
        self.text.replace_range(a..b, &value);
        self.caret = a + value.len();
        self.anchor = self.caret;
    }
}

impl WinState {
    pub(super) fn zoom_box(&self) -> Option<Rect> {
        let (top, height) = if self.show_toolbar && self.toolbar_position() == 0 {
            (self.toolbar_top(), toolbar::thickness(false))
        } else if self.show_formatting_toolbar {
            (
                self.formatting_toolbar_top(),
                bars::formatting_toolbar_height(),
            )
        } else {
            return None;
        };
        Some(Rect::new(
            self.width - WIDTH - 8.0,
            top + 3.0,
            WIDTH,
            (height - 6.0).max(20.0),
        ))
    }

    pub(super) fn begin_zoom_edit(&mut self) {
        let text = percent_text(self.document_zoom as f64 * 100.0);
        let end = text.len();
        self.zoom_edit = Some(Edit {
            text,
            anchor: 0,
            caret: end,
        });
        for (_, d) in &mut self.dialogs {
            if d.modeless {
                d.inactive = true;
            }
        }
    }

    pub(super) fn apply_zoom(&mut self, percent: f64) {
        if !percent.is_finite() {
            return;
        }
        let percent = (percent.clamp(MIN, MAX) * 100.0).round() / 100.0;
        self.document_zoom = (percent / 100.0) as f32;
        self.zoom_edit = None;
        self.view.status = None;
        crate::settings::update(|s| s.document_zoom = percent);
    }

    /// Zoom by `factor` about the pointer, `cy` down the window, as a pinch
    /// or ⌘-scroll does: the line under the pointer stays under it. `save`
    /// keeps the new zoom for next time; a gesture saves once, at its end.
    pub(super) fn zoom_at(&mut self, factor: f64, cy: f32, save: bool) {
        let old = self.document_zoom;
        let percent = (old as f64 * 100.0 * factor).clamp(MIN, MAX);
        if !percent.is_finite() {
            return;
        }
        let new = (percent / 100.0) as f32;
        if save {
            self.apply_zoom(percent);
        } else {
            self.document_zoom = new;
            self.zoom_edit = None;
        }
        let new = self.document_zoom;
        let v = self.main_view_mut();
        // How far below the top of the text the pointer is, in document
        // units, before and after.
        let below = (cy / old - v.area.y).max(0.0);
        v.scroll_y = (v.scroll_y + below - below * old / new).max(0.0);
    }

    pub(super) fn commit_zoom(&mut self) -> bool {
        let Some(edit) = &self.zoom_edit else {
            return true;
        };
        if let Some(n) = parse(&edit.text) {
            self.apply_zoom(n);
            true
        } else {
            self.view.status = Some("Enter a zoom percentage from 10% to 800%.".into());
            false
        }
    }

    pub(super) fn zoom_text(&mut self, text: &str) {
        if let Some(edit) = &mut self.zoom_edit {
            edit.replace(text);
        }
    }

    pub(super) fn zoom_selection(&self) -> Option<String> {
        let e = self.zoom_edit.as_ref()?;
        let (a, b) = (e.anchor.min(e.caret), e.anchor.max(e.caret));
        (a != b).then(|| e.text[a..b].to_string())
    }

    pub(super) fn zoom_key(&mut self, key: DKey, text: Option<&str>) {
        match key {
            DKey::Return | DKey::Tab { .. } => {
                self.commit_zoom();
            }
            DKey::Escape => {
                self.zoom_edit = None;
            }
            _ => {
                if let Some(e) = &mut self.zoom_edit {
                    match key {
                        DKey::SelectAll => {
                            e.anchor = 0;
                            e.caret = e.text.len();
                        }
                        DKey::Backspace => {
                            if e.anchor == e.caret {
                                e.anchor = e.caret.saturating_sub(1);
                            }
                            e.replace("");
                        }
                        DKey::Delete => {
                            if e.anchor == e.caret {
                                e.anchor = (e.caret + 1).min(e.text.len());
                            }
                            e.replace("");
                        }
                        DKey::Left => {
                            e.caret = if e.anchor != e.caret {
                                e.anchor.min(e.caret)
                            } else {
                                e.caret.saturating_sub(1)
                            };
                            e.anchor = e.caret;
                        }
                        DKey::Right => {
                            e.caret = if e.anchor != e.caret {
                                e.anchor.max(e.caret)
                            } else {
                                (e.caret + 1).min(e.text.len())
                            };
                            e.anchor = e.caret;
                        }
                        DKey::Char => {
                            if let Some(text) = text {
                                e.replace(text);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub(super) fn choose_zoom(&mut self, choice: u8) {
        if let Some(&n) = PRESETS.get(choice as usize) {
            self.apply_zoom(n);
            return;
        }
        if choice == 5 {
            self.begin_zoom_edit();
            return;
        }
        let current = self.main_view().visible_page();
        let v = self.main_view();
        let (pw, ph) = v.page_size();
        let space =
            if choice == 6 { v.area.w } else { v.area.h } as f64 * self.document_zoom as f64;
        let page = if choice == 6 { pw } else { ph } as f64;
        let n = space / (page + 2.0 * crate::docview::PAGE_GAP as f64) * 100.0;
        self.apply_zoom(n);
        let v = self.main_view_mut();
        v.page_view = true;
        v.headers_wanted = true;
        v.show_page(current);
    }

    pub(super) fn zoom_click(&mut self, x: f32, y: f32, label: &mut Label) -> bool {
        let Some(r) = self.zoom_box() else {
            return false;
        };
        if !r.contains(x, y) {
            return false;
        }
        if x < r.right() - 24.0 {
            self.begin_zoom_edit();
            return true;
        }
        if !self.commit_zoom() {
            return true;
        }
        let labels = [
            "50%",
            "75%",
            "100%",
            "125%",
            "150%",
            "Custom…",
            "Fill Horizontal",
            "Fill Vertical",
        ];
        let mut items = Vec::new();
        let mut actions = Vec::new();
        for (i, name) in labels.iter().enumerate() {
            if i == 6 {
                items.push(menu::Item::separator());
            }
            items.push(
                menu::Item::new(*name, i as u32).checked(
                    PRESETS
                        .get(i)
                        .is_some_and(|n| (*n - self.document_zoom as f64 * 100.0).abs() < 0.005),
                ),
            );
            actions.push(Action::Zoom(i as u8));
        }
        let m = Menu {
            title: "Zoom".into(),
            items,
        };
        let open = menu::place(
            label,
            &m,
            r,
            Rect::new(0.0, 0.0, self.width, self.height),
            true,
        );
        self.menu = Some(MenuTrack {
            source: MenuSource::Zoom,
            menus: vec![m],
            actions,
            open,
            submenu: None,
            opened: Instant::now(),
            held: true,
        });
        true
    }

    pub(super) fn draw_zoom(&self, c: &mut Canvas, label: &mut Label) {
        let Some(r) = self.zoom_box() else {
            return;
        };
        let font = quill_layout::fonts::UI_FONT;
        c.fill(
            Rect::new(r.x - 8.0, r.y - 3.0, self.width - r.x + 8.0, r.h + 6.0),
            bars::background(),
        );
        round_rect(
            c,
            r,
            4.0,
            if self.zoom_edit.is_some() {
                0x0A84FF
            } else {
                0xB8B8B8
            },
        );
        let hair = c.hair();
        round_rect(c, r.inset(hair), 3.0, color::WHITE);
        c.vline(r.right() - 24.0, r.y + 2.0, r.bottom() - 2.0, 0xDDDDDD);
        label.draw(
            c,
            font,
            11.0,
            false,
            r.right() - 17.0,
            r.y + 14.0,
            "▾",
            0x444444,
        );
        let field_clip = c.set_clip(Rect::new(r.x + 4.0, r.y + 1.0, r.w - 32.0, r.h - 2.0));
        let text = self.zoom_edit.as_ref().map_or_else(
            || percent_text(self.document_zoom as f64 * 100.0),
            |e| e.text.clone(),
        );
        if let Some(e) = &self.zoom_edit {
            let (a, b) = (e.anchor.min(e.caret), e.anchor.max(e.caret));
            let start = label.width(font, 12.0, false, &text[..a]);
            let end = label.width(font, 12.0, false, &text[..b]);
            c.fill(
                Rect::new(r.x + 6.0 + start, r.y + 3.0, end - start, r.h - 6.0),
                0xC5DFFF,
            );
            let caret = label.width(font, 12.0, false, &text[..e.caret]);
            c.vline(r.x + 6.0 + caret, r.y + 3.0, r.bottom() - 3.0, 0x333333);
        }
        label.draw(c, font, 12.0, false, r.x + 6.0, r.y + 15.0, &text, 0x202020);
        c.restore_clip(field_clip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentage_input_rejects_invalid_and_rounds_to_display_precision() {
        assert_eq!(parse("125.25%"), Some(125.25));
        assert_eq!(parse(" 75 % "), Some(75.0));
        for text in ["", "NaN", "inf", "9", "801", "100%%"] {
            assert_eq!(parse(text), None, "{text}");
        }
        assert_eq!(percent_text(100.0), "100%");
        assert_eq!(percent_text(63.75), "63.75%");
    }

    #[test]
    fn toolbar_zoom_editing_does_not_change_document_or_its_undo_history() {
        let mut state = WinState::new(DocView::new(
            quill_core::text::from_plain_text("Keep this document"),
            "Zoom".into(),
        ));
        let before = state.view.doc.main.clone();
        let was_modified = state.view.doc.is_modified();
        let undo = state.view.doc.undo_label().map(str::to_string);
        let mut clip = Clipboard::new();
        state.begin_zoom_edit();
        state.zoom_key(DKey::Char, Some("125.5%"));
        state.zoom_key(DKey::Return, None);
        assert!((state.document_zoom - 1.255).abs() < 0.00001);
        let saved = crate::settings::get().to_text();
        assert_eq!(
            crate::settings::Settings::from_text(&saved).document_zoom,
            125.5
        );
        state.begin_zoom_edit();
        state.act(Action::Cmd(Cmd::SelectAll), &mut clip);
        state.zoom_text("9%");
        assert!(!state.commit_zoom());
        assert!(state.zoom_edit.is_some());
        state.zoom_key(DKey::Escape, None);
        assert!((state.document_zoom - 1.255).abs() < 0.00001);
        for (i, n) in PRESETS.iter().enumerate() {
            state.choose_zoom(i as u8);
            assert!((state.document_zoom as f64 * 100.0 - n).abs() < 0.0001);
        }
        assert_eq!(state.view.doc.main, before);
        assert_eq!(state.view.doc.is_modified(), was_modified);
        assert_eq!(state.view.doc.undo_label().map(str::to_string), undo);
    }

    #[test]
    fn fit_uses_visible_page_not_caret_or_total_document_height() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let doc =
            quill_core::text::from_plain_text(&"A paragraph on a long document.\n".repeat(260));
        let mut state = WinState::new(DocView::new(doc, "Pages".into()));
        state.document_zoom = 1.0;
        state.view.page_view = true;
        state.view.area = Rect::new(0.0, 100.0, 800.0, 550.0);
        state.view.update_layout(&mut fonts);
        assert!(state.view.pages.len() > 3);
        state.view.show_page(3);
        assert_eq!(state.view.caret_page(), 0);
        assert_eq!(state.view.visible_page(), 3);
        let (_, height) = state.view.page_size();
        let expected = (550.0 / (height as f64 + 2.0 * crate::docview::PAGE_GAP as f64) * 10000.0)
            .round()
            / 100.0;
        state.choose_zoom(7);
        assert!((state.document_zoom as f64 * 100.0 - expected).abs() < 0.0001);
        assert_eq!(
            percent_text(state.document_zoom as f64 * 100.0),
            percent_text(expected)
        );
        assert_eq!(state.view.visible_page(), 3);
        let restored = crate::settings::Settings::from_text(&crate::settings::get().to_text());
        assert_eq!(restored.document_zoom, expected);
        state.document_zoom = 1.25;
        state.view.area.w = 400.0;
        let (width, _) = state.view.page_size();
        let expected = (500.0 / (width as f64 + 2.0 * crate::docview::PAGE_GAP as f64) * 10000.0)
            .round()
            / 100.0;
        state.choose_zoom(6);
        assert!((state.document_zoom as f64 * 100.0 - expected).abs() < 0.0001);
        assert_eq!(state.view.visible_page(), 3);
    }

    #[test]
    fn clicking_the_zoom_dropdown_applies_the_clicked_preset() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut cache = GlyphCache::new();
        let mut label = Label {
            fonts: &mut fonts,
            cache: &mut cache,
        };
        let mut state = WinState::new(DocView::new(Document::new(), "Zoom".into()));
        state.width = 900.0;
        state.height = 700.0;
        state.ui = 1.0;
        let r = state.zoom_box().unwrap();
        assert!(state.zoom_click(r.right() - 10.0, r.y + 10.0, &mut label));
        let m = state.menu.as_ref().unwrap();
        let row = menu::item_rect(m.menu(), &m.open, 2).unwrap();
        let (x, y) = (row.x + row.w / 2.0, row.y + row.h / 2.0);
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        let mut clip = Clipboard::new();
        state.mouse_down(x, y, Mods::default(), &mut label, &env, &mut clip);
        state.mouse_up(x, y, &mut clip);
        assert_eq!(state.document_zoom, 1.0);
        assert!(state.menu.is_none());
        assert!(!state.view.doc.is_modified());
    }

    #[test]
    fn preview_command_shortcut_and_saved_toolbar_route_to_platform_preview() {
        let mut view = DocView::new(Document::new(), "Preview".into());
        let mut clip = Clipboard::new();
        assert_eq!(
            view.exec(Cmd::PrintPreview, false, &mut clip),
            Effect::NativePreview
        );
        let cmd = command_map::lookup(
            &Key::Character("i".into()),
            Some('i'),
            Mods {
                cmd: true,
                opt: true,
                ..Default::default()
            },
        );
        assert_eq!(cmd, Some(Cmd::PrintPreview));
        crate::settings::update(|s| s.toolbar_buttons = vec![(0, 23, "PrintPreview".into())]);
        assert_eq!(toolbar::command(0), Some(Cmd::PrintPreview));
        assert_eq!(view.exec(Cmd::Print, false, &mut clip), Effect::Print);
        assert_eq!(view.exec(Cmd::SavePdf, false, &mut clip), Effect::SavePdf);
    }
    #[test]
    fn pinch_zoom_keeps_the_line_under_the_pointer() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        state.document_zoom = 1.0;
        state.view.area.y = 40.0;
        state.view.scroll_y = 100.0;
        let under = |s: &WinState, cy: f32| cy / s.document_zoom - s.view.area.y + s.view.scroll_y;
        let before = under(&state, 300.0);
        state.zoom_at(2.0, 300.0, false);
        assert!((state.document_zoom - 2.0).abs() < 1e-6);
        // The view's top edge stays put on screen, so its document-unit
        // position halves with the zoom.
        state.view.area.y = 20.0;
        assert!((under(&state, 300.0) - before).abs() < 1e-3);
        state.zoom_at(1000.0, 300.0, false);
        assert!((state.document_zoom as f64 * 100.0 - MAX).abs() < 1e-3);
    }
}
