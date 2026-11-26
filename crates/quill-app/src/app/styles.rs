//! The Style Sheets palette in a window: where it sits, its clicks and its
//! buttons. The list and drawing are in `styles_palette`.

use super::*;
use crate::styles_palette::{self as sp, Button, Entry, Hit, Tab};

impl WinState {
    /// Whether the palette is showing in this window (View > Stylesheets
    /// is for every window).
    pub(super) fn palette_shown() -> bool {
        crate::settings::with(|s| s.show_styles_palette)
    }

    /// The width the docked palette takes from the right of the text area.
    pub(super) fn palette_dock_width(&self) -> f32 {
        if Self::palette_shown() && crate::settings::with(|s| s.styles_palette_docked) {
            self.palette
                .dock_width
                .min((self.width - 220.0).max(sp::DOCK_MIN_WIDTH))
        } else {
            0.0
        }
    }

    /// The palette's rectangle in this window: the column left of the
    /// right-hand toolbar when it is docked, or the whole of its own window
    /// (whose size the caller has set in `palette.window`).
    pub(super) fn palette_bounds(&mut self) -> Option<Rect> {
        if !Self::palette_shown() {
            return None;
        }
        self.palette.tab = Tab::from_index(crate::settings::with(|s| s.styles_palette_tab));
        self.palette.show_all = crate::settings::with(|s| s.styles_palette_all);
        self.palette.docked = crate::settings::with(|s| s.styles_palette_docked);
        if let Some((w, h)) = self.palette.window {
            return Some(Rect::new(0.0, 0.0, w, h));
        }
        if !self.palette.docked {
            return None;
        }
        let top = self.text_top();
        let bottom = self.height - BAR;
        let (x0, narrower) = self.toolbar_side();
        let right = self.width - self.right_gutter() - (narrower - x0);
        let width = self.palette_dock_width();
        Some(Rect::new(
            right - width,
            top,
            width,
            (bottom - top).max(0.0),
        ))
    }

    fn palette_edge(&self) -> f32 {
        let (x, right) = self.toolbar_side();
        self.width - self.right_gutter() - (right - x)
    }

    pub(super) fn palette_resize_hit(&self, x: f32, y: f32) -> bool {
        let width = self.palette_dock_width();
        width > 0.0
            && y >= self.text_top()
            && y < self.height - BAR
            && (x - (self.palette_edge() - width)).abs() <= 4.0
    }

    pub(super) fn start_palette_resize(&mut self, x: f32, y: f32) -> bool {
        if !self.palette_resize_hit(x, y) {
            return false;
        }
        self.palette.resize_grab = Some(self.palette_dock_width() + x - self.palette_edge());
        true
    }

    pub(super) fn resize_palette(&mut self, x: f32) -> bool {
        let Some(grab) = self.palette.resize_grab else {
            return false;
        };
        self.palette.dock_width = (self.palette_edge() - x + grab).clamp(
            sp::DOCK_MIN_WIDTH,
            (self.width - 220.0).max(sp::DOCK_MIN_WIDTH),
        );
        true
    }

    /// The styles at the insertion point of the view being typed in (the
    /// pane's, while it has focus), which is the view a row click applies to.
    fn palette_current(&self) -> sp::Current {
        let v = &self.view;
        sp::Current {
            paragraph: v.current_pap().style,
            character: v.current_chp().character_style,
        }
    }

    pub(super) fn draw_palette(&mut self, c: &mut Canvas, label: &mut Label) {
        let Some(b) = self.palette_bounds() else {
            return;
        };
        let cur = self.palette_current();
        let rows = sp::entries(
            &self.main_view().doc,
            self.palette.tab,
            self.palette.show_all,
        );
        let max = sp::max_scroll(b, self.palette.window.is_some(), rows.len());
        self.palette.scroll = self.palette.scroll.min(max);
        if self
            .palette
            .chosen
            .is_some_and(|entry| !rows.contains(&entry))
        {
            self.palette.chosen = None;
            self.palette.actions_at = None;
        }
        sp::draw(
            c,
            label,
            &self.palette,
            b,
            &self.main_view().doc,
            cur,
            &rows,
        );
    }

    pub fn toggle_palette(&mut self) {
        crate::settings::update(|s| s.show_styles_palette = !s.show_styles_palette);
    }

    pub(super) fn set_palette_docked(&mut self, docked: bool) {
        crate::settings::update(|s| s.styles_palette_docked = docked);
        self.palette.docked = docked;
    }

    pub(super) fn palette_context(&mut self, x: f32, y: f32) -> bool {
        let Some(b) = self.palette_bounds() else {
            return false;
        };
        let rows = sp::entries(
            &self.main_view().doc,
            self.palette.tab,
            self.palette.show_all,
        );
        let hit = sp::hit(
            b,
            self.palette.window.is_some(),
            self.palette.scroll,
            rows.len(),
            x,
            y,
        );
        if let Some(Hit::Row(i)) = hit {
            self.palette.chosen = Some(rows[i]);
            self.palette.actions_at = Some((x, y));
            self.palette.last_row_click = None;
        }
        hit.is_some()
    }

    /// A press at (`cx`, `cy`). Some when the palette took it.
    pub(super) fn palette_click(
        &mut self,
        cx: f32,
        cy: f32,
        env: &Env,
        clip: &mut Clipboard,
    ) -> Option<Effect> {
        let b = self.palette_bounds()?;
        let rows = sp::entries(
            &self.main_view().doc,
            self.palette.tab,
            self.palette.show_all,
        );
        let os = self.palette.window.is_some();
        if let Some(anchor) = self.palette.actions_at.take() {
            if let Some(button) = sp::action_at(b, anchor, cx, cy) {
                return Some(self.palette_button(button, env));
            }
            // Dismiss without applying a style beneath the menu.
            return Some(Effect::None);
        }
        let hit = sp::hit(b, os, self.palette.scroll, rows.len(), cx, cy)?;
        if !matches!(hit, Hit::Row(_)) {
            self.palette.last_row_click = None;
        }
        match hit {
            Hit::More => self.palette.actions_at = Some((b.right(), b.bottom() - 22.0)),
            Hit::Close => self.toggle_palette(),
            Hit::Dock => self.set_palette_docked(true),
            Hit::Title => self.palette.title_press = Some((cx, cy)),
            Hit::ToggleScope => {
                self.palette.show_all = !self.palette.show_all;
                crate::settings::update(|s| s.styles_palette_all = self.palette.show_all);
                self.palette.set_tab(self.palette.tab);
            }
            Hit::Tab(tab) => {
                crate::settings::update(|s| s.styles_palette_tab = tab.index());
                self.palette.set_tab(tab);
            }
            Hit::Button(button) => return Some(self.palette_button(button, env)),
            Hit::Row(i) => {
                let entry = rows[i];
                self.palette.chosen = Some(entry);
                let now = Instant::now();
                if sp::can_edit(Some(entry))
                    && self
                        .palette
                        .last_row_click
                        .take()
                        .is_some_and(|(last, at)| {
                            last == entry && now.duration_since(at) < Duration::from_millis(400)
                        })
                {
                    return Some(self.palette_button(Button::Edit, env));
                }
                self.palette.last_row_click = Some((entry, now));
                let action = match entry {
                    // Back to Normal with no character styles, whatever the
                    // paragraph looked like (and without the question about
                    // reapplying a reformatted paragraph's own style).
                    Entry::NoParagraph => {
                        self.view.clear_styles();
                        return Some(Effect::None);
                    }
                    Entry::ClearParagraph => {
                        self.view.clear_formatting();
                        return Some(Effect::None);
                    }
                    Entry::ClearCharacter => {
                        self.view.clear_character_formatting();
                        return Some(Effect::None);
                    }
                    Entry::Paragraph(id) => Action::Style(id),
                    Entry::Character(id) => Action::CharacterStyle(id),
                };
                return Some(self.act(action, clip));
            }
            Hit::Chrome => {}
        }
        Some(Effect::None)
    }

    /// Tear off the docked palette once the title moves beyond a click.
    pub(super) fn tear_off_palette(&mut self, x: f32, y: f32) -> bool {
        let Some((start_x, start_y)) = self.palette.title_press else {
            return false;
        };
        if (x / self.ui - start_x).hypot(y / self.ui - start_y) < 12.0 {
            return false;
        }
        self.palette.title_press = None;
        if self.palette.window.is_some() || self.palette_bounds().is_none() {
            return false;
        }
        self.set_palette_docked(false);
        true
    }

    /// Move the pointer over the palette. True when the window needs
    /// drawing again.
    pub(super) fn palette_move(&mut self, cx: f32, cy: f32) -> bool {
        let Some(b) = self.palette_bounds() else {
            return false;
        };
        let rows = sp::entries(
            &self.main_view().doc,
            self.palette.tab,
            self.palette.show_all,
        )
        .len();
        let os = self.palette.window.is_some();
        let hover = if let Some(anchor) = self.palette.actions_at {
            sp::action_at(b, anchor, cx, cy).map(Hit::Button)
        } else {
            sp::hit(b, os, self.palette.scroll, rows, cx, cy)
        };
        let changed = hover != self.palette.hover;
        self.palette.hover = hover;
        changed
    }

    /// A wheel turn over the palette scrolls its list.
    pub(super) fn palette_wheel(&mut self, cx: f32, cy: f32, dy: f32) -> bool {
        let Some(b) = self.palette_bounds() else {
            return false;
        };
        if !b.contains(cx, cy) {
            return false;
        }
        let rows = sp::entries(
            &self.main_view().doc,
            self.palette.tab,
            self.palette.show_all,
        )
        .len();
        let max = sp::max_scroll(b, self.palette.window.is_some(), rows);
        self.palette.scroll = (self.palette.scroll - dy / self.ui).clamp(0.0, max);
        true
    }

    /// The row Edit, Copy and Delete work on: the last one clicked, or else
    /// the paragraph's style.
    fn palette_target(&self) -> Entry {
        self.palette
            .chosen
            .unwrap_or(Entry::Paragraph(self.view.current_pap().style))
    }

    fn palette_button(&mut self, button: Button, env: &Env) -> Effect {
        // The style sheet belongs to the main text; a header or footer pane
        // only has a copy of it. So the buttons work there. The target is
        // read first: focusing the main text swaps the view, and the buttons
        // were drawn for the pane's paragraph style.
        let target = self.palette_target();
        self.focus_main();
        let doc = &self.view.doc;
        match button {
            Button::Edit | Button::Copy if !sp::can_edit(Some(target)) => return Effect::None,
            Button::Delete if !sp::can_delete(doc, Some(target)) => return Effect::None,
            _ => {}
        }
        // The tab says which kind New makes; on All it follows the row.
        let character = match self.palette.tab {
            Tab::Character => true,
            Tab::Paragraph => false,
            Tab::All => matches!(target, Entry::Character(_)),
        };
        match button {
            Button::New if character => self.palette_character_editor(None, env),
            Button::New => self.palette_style_dialog(None, env),
            Button::Edit => match target {
                Entry::Character(id) => self.palette_character_editor(Some(id), env),
                Entry::Paragraph(id) => self.palette_style_dialog(Some(id), env),
                Entry::NoParagraph | Entry::ClearParagraph | Entry::ClearCharacter => {}
            },
            Button::Copy => self.palette_copy(target),
            Button::Delete => self.palette_delete(target),
        }
        Effect::None
    }

    fn palette_character_editor(&mut self, id: Option<u16>, env: &Env) {
        self.font_names = env.font_names.to_vec();
        self.font_families = env.font_families.to_vec();
        let editor = crate::character_style_dialog::editor(&self.view, &self.font_families, id);
        self.dialogs.push((Which::CharacterStyleEdit(id), editor));
    }

    /// The paragraph style editor on style `id`, or on a new style.
    fn palette_style_dialog(&mut self, id: Option<quill_core::props::StyleId>, env: &Env) {
        self.font_names = env.font_names.to_vec();
        self.font_families = env.font_families.to_vec();
        self.open_style_editor(id);
    }

    fn palette_copy(&mut self, target: Entry) {
        let view = &mut self.view;
        let made = match target {
            Entry::NoParagraph | Entry::ClearParagraph | Entry::ClearCharacter => None,
            Entry::Character(id) => {
                crate::character_style_dialog::duplicate(view, id).map(Entry::Character)
            }
            Entry::Paragraph(id) => {
                Some(Entry::Paragraph(crate::style_dialog::duplicate(view, id)))
            }
        };
        if made.is_some() {
            self.palette.chosen = made;
        }
    }

    fn palette_delete(&mut self, target: Entry) {
        match target {
            Entry::NoParagraph | Entry::ClearParagraph | Entry::ClearCharacter => {}
            Entry::Character(id) => {
                crate::character_style_dialog::delete(&mut self.view, id);
            }
            // A style in use asks what its paragraphs become.
            Entry::Paragraph(id) => self.delete_style_asking(id),
        }
        self.palette.chosen = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::character_styles::CharacterFormat;

    fn state() -> WinState {
        let mut doc = quill_core::text::from_plain_text("Body text");
        let bold = quill_core::props::CharProps {
            bold: true,
            ..Default::default()
        };
        doc.add_character_style("Strong", CharacterFormat::new(bold, 1 << 2))
            .unwrap();
        doc.forget_history();
        let mut st = WinState::new(DocView::new(doc, "Doc".into()));
        st.width = 900.0;
        st.height = 700.0;
        st.ui = 1.0;
        crate::settings::update(|s| {
            s.show_styles_palette = true;
            s.styles_palette_docked = true;
            s.styles_palette_tab = 0;
            s.styles_palette_all = false;
        });
        st
    }

    fn click(st: &mut WinState, hit: Hit) {
        let b = st.palette_bounds().expect("the palette is showing");
        let (x, y) = match hit {
            Hit::Button(button @ (Button::Edit | Button::Copy)) => {
                click(st, Hit::More);
                sp::action_point(b, st.palette.actions_at.unwrap(), button)
            }
            _ => sp::point_of(b, st.palette.window.is_some(), hit),
        };
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        let mut clip = Clipboard::new();
        assert!(st.palette_click(x, y, &env, &mut clip).is_some());
    }

    #[test]
    fn context_selects_a_style_for_editing_without_applying_it() {
        let mut st = state();
        click(&mut st, Hit::Tab(Tab::Character));
        let b = st.palette_bounds().unwrap();
        let (x, y) = sp::point_of(b, false, Hit::Row(2));
        assert!(st.palette_context(x, y));
        assert_eq!(st.palette.chosen, Some(Entry::Character(1)));
        assert!(!st.view.doc.is_modified());
        let (x, y) = sp::action_point(b, st.palette.actions_at.unwrap(), Button::Edit);
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        assert!(
            st.palette_click(x, y, &env, &mut Clipboard::new())
                .is_some()
        );
        assert!(matches!(
            st.dialogs.last(),
            Some((Which::CharacterStyleEdit(_), _))
        ));
    }

    #[test]
    fn a_docked_palette_takes_its_width_from_the_text_area() {
        let mut st = state();
        let (_, with) = st.side_toolbar();
        crate::settings::update(|s| s.styles_palette_docked = false);
        let (_, floating) = st.side_toolbar();
        crate::settings::update(|s| s.show_styles_palette = false);
        let (_, hidden) = st.side_toolbar();
        assert_eq!(with - floating, sp::DOCK_WIDTH);
        assert_eq!(floating, hidden);
        assert!(st.palette_bounds().is_none());
    }

    #[test]
    fn tabs_are_remembered_in_the_settings() {
        let mut st = state();
        click(&mut st, Hit::Tab(Tab::Character));
        assert_eq!(crate::settings::with(|s| s.styles_palette_tab), 2);
        let b = st.palette_bounds().unwrap();
        let rows = sp::entries(&st.view.doc, st.palette.tab, st.palette.show_all);
        assert!(
            rows.iter()
                .all(|e| matches!(e, Entry::Character(_) | Entry::ClearCharacter))
        );
        assert!(b.w > 0.0);
    }

    #[test]
    fn applying_a_hidden_style_from_all_styles_reveals_it_in_document_styles() {
        let mut st = state();
        let header = Entry::Paragraph(st.view.doc.styles.find("header").unwrap());
        click(&mut st, Hit::Tab(Tab::Paragraph));
        assert!(!sp::entries(&st.view.doc, Tab::Paragraph, false).contains(&header));

        click(&mut st, Hit::ToggleScope);
        assert!(crate::settings::with(|s| s.styles_palette_all));
        let all = sp::entries(&st.view.doc, Tab::Paragraph, st.palette.show_all);
        let row = all.iter().position(|&entry| entry == header).unwrap();
        click(&mut st, Hit::Row(row));

        click(&mut st, Hit::ToggleScope);
        assert!(!crate::settings::with(|s| s.styles_palette_all));
        assert!(sp::entries(&st.view.doc, Tab::Paragraph, st.palette.show_all).contains(&header));
    }

    #[test]
    fn clicking_rows_applies_the_style() {
        let mut st = state();
        click(&mut st, Hit::Tab(Tab::Character));
        st.view.set_selection(quill_core::Selection {
            anchor: quill_core::Position::new(0, 0),
            focus: quill_core::Position::new(0, 4),
        });
        // Row 0 is No Style, row 1 is Clear Formatting, row 2 is Strong.
        click(&mut st, Hit::Row(2));
        assert_eq!(st.view.doc.main.paras[0].chp_at(2).character_style, 1);
        click(&mut st, Hit::Row(0));
        assert_eq!(st.view.doc.main.paras[0].chp_at(2).character_style, 0);
        click(&mut st, Hit::Tab(Tab::Paragraph));
        // Rows 0 to 2 are No Style, Clear Formatting and Normal; row 3 is the
        // first other style.
        click(&mut st, Hit::Row(3));
        assert_eq!(
            st.view.current_pap().style,
            sp::entries(&st.view.doc, Tab::Paragraph, false)
                .iter()
                .find_map(|e| match e {
                    Entry::Paragraph(id) if *id != 0 => Some(*id),
                    _ => None,
                })
                .unwrap()
        );
    }

    #[test]
    fn no_style_takes_the_paragraph_style_and_character_styles_off_the_selection() {
        let mut st = state();
        let heading = st.view.doc.styles.find("heading 1").unwrap();
        st.view.set_style(heading);
        st.view.set_selection(quill_core::Selection {
            anchor: quill_core::Position::new(0, 0),
            focus: quill_core::Position::new(0, 4),
        });
        st.view.set_character_style(1);
        assert_eq!(st.view.doc.main.paras[0].chp_at(2).character_style, 1);
        // Row 0 of the Paragraph tab is No Style.
        click(&mut st, Hit::Tab(Tab::Paragraph));
        click(&mut st, Hit::Row(0));
        assert_eq!(st.view.current_pap().style, 0);
        assert_eq!(st.view.doc.main.paras[0].chp_at(2).character_style, 0);
        // One click, one step: a single Undo puts both back.
        st.view.doc.undo();
        assert_eq!(st.view.doc.main.paras[0].pap.style, heading);
        assert_eq!(st.view.doc.main.paras[0].chp_at(2).character_style, 1);
        // It is not a style: nothing to edit, copy or delete.
        assert!(!sp::can_edit(st.palette.chosen));
        assert!(!sp::can_delete(&st.view.doc, st.palette.chosen));
    }

    #[test]
    fn copy_and_delete_work_on_the_chosen_character_style() {
        let mut st = state();
        click(&mut st, Hit::Tab(Tab::Character));
        click(&mut st, Hit::Row(3));
        click(&mut st, Hit::Button(Button::Copy));
        let names: Vec<String> = st
            .view
            .doc
            .styles
            .characters
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert_eq!(names, ["Normal", "Strong", "Strong copy"]);
        assert_eq!(st.palette.chosen, Some(Entry::Character(3)));
        click(&mut st, Hit::Button(Button::Delete));
        assert_eq!(st.view.doc.styles.characters.len(), 2);
        assert_eq!(st.palette.chosen, None);
        // Undo brings the copy back.
        st.view.doc.undo();
        assert_eq!(st.view.doc.styles.characters.len(), 3);
    }

    #[test]
    fn buttons_work_on_the_header_style_when_a_header_has_focus() {
        let mut st = state();
        // A user-made style on the main paragraph, the built-in style in the
        // header. With nothing chosen, Delete must not touch the main one.
        let made = crate::style_dialog::duplicate(&mut st.view, 0);
        st.view.set_style(made);
        st.open_pane(quill_core::StoryId::Header);
        st.focus_pane();
        let header_style = st.view.current_pap().style;
        assert_ne!(header_style, made);
        assert!(st.view.doc.styles.get(header_style).builtin);
        st.palette.chosen = None;
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        st.palette_button(Button::Delete, &env);
        // The header style is built in, so Delete does nothing. Acting on the
        // main paragraph's style would ask what its paragraphs become.
        assert!(st.dialogs.is_empty());
        st.focus_main();
        assert_eq!(st.view.current_pap().style, made);
        assert!(!st.view.doc.styles.get(made).name.is_empty());
    }

    #[test]
    fn deleting_a_character_style_keeps_typing_at_the_caret_in_its_own_style() {
        let mut st = state();
        let bold = quill_core::props::CharProps {
            bold: true,
            ..Default::default()
        };
        let accent = st
            .view
            .doc
            .add_character_style("Accent", CharacterFormat::new(bold, 1 << 2))
            .unwrap();
        st.view
            .set_selection(quill_core::Selection::caret(quill_core::Position::new(
                0, 4,
            )));
        st.view.set_character_style(accent);
        // Deleting Strong moves Accent down one ID, and the caret follows it.
        st.palette.chosen = Some(Entry::Character(accent - 1));
        click(&mut st, Hit::Button(Button::Delete));
        assert_eq!(st.view.doc.styles.characters[1].name, "Accent");
        assert_eq!(st.view.current_chp().character_style, accent - 1);
    }

    #[test]
    fn paragraph_copies_are_named_like_character_copies_and_follow_themselves() {
        let mut st = state();
        let body = st.view.doc.add_style(
            "Body",
            0,
            None,
            quill_core::props::ParaProps::default(),
            quill_core::props::CharProps::default(),
        );
        st.palette.chosen = Some(Entry::Paragraph(body));
        click(&mut st, Hit::Button(Button::Copy));
        let Some(Entry::Paragraph(copy)) = st.palette.chosen else {
            panic!("the copy is chosen");
        };
        assert_eq!(st.view.doc.styles.get(copy).name, "Body copy");
        // Body is followed by Body, so the copy is followed by the copy.
        assert_eq!(st.view.doc.styles.get(copy).next, copy);
        // Copying the copy does not make "Body copy copy".
        click(&mut st, Hit::Button(Button::Copy));
        let Some(Entry::Paragraph(again)) = st.palette.chosen else {
            panic!("the second copy is chosen");
        };
        assert_eq!(st.view.doc.styles.get(again).name, "Body copy 2");
    }

    #[test]
    fn new_and_edit_open_the_editors() {
        let mut st = state();
        click(&mut st, Hit::Tab(Tab::Character));
        click(&mut st, Hit::Button(Button::New));
        assert!(matches!(
            st.dialogs.last(),
            Some((Which::CharacterStyleEdit(None), _))
        ));
        st.dialogs.clear();
        click(&mut st, Hit::Row(2));
        click(&mut st, Hit::Button(Button::Edit));
        assert!(matches!(
            st.dialogs.last(),
            Some((Which::CharacterStyleEdit(Some(1)), _))
        ));
        st.dialogs.clear();
        click(&mut st, Hit::Tab(Tab::Paragraph));
        click(&mut st, Hit::Button(Button::New));
        assert!(matches!(st.dialogs.last(), Some((Which::Style(_), _))));
    }

    #[test]
    fn the_dock_button_moves_the_palette_between_the_window_and_a_window_of_its_own() {
        let mut st = state();
        let b = st.palette_bounds().unwrap();
        let (x, y) = sp::point_of(b, false, Hit::Title);
        click(&mut st, Hit::Title);
        assert!(!st.tear_off_palette(x + 3.0, y));
        assert!(st.tear_off_palette(x - 20.0, y));
        assert!(!crate::settings::with(|s| s.styles_palette_docked));
        // Docked off, this window draws and reserves nothing.
        assert!(st.palette_bounds().is_none());
        assert_eq!(st.side_toolbar().1, st.toolbar_side().1);
        // The palette's own window draws it whole, with no title bar.
        st.palette.window = Some((220.0, 340.0));
        assert_eq!(st.palette_bounds(), Some(Rect::new(0.0, 0.0, 220.0, 340.0)));
        click(&mut st, Hit::Tab(Tab::Character));
        if cfg!(target_os = "macos") {
            st.set_palette_docked(true);
        } else {
            click(&mut st, Hit::Dock);
        }
        assert!(crate::settings::with(|s| s.styles_palette_docked));
    }

    #[test]
    fn dragging_the_docked_palette_edge_changes_its_width() {
        let mut st = state();
        let before = st.palette_bounds().unwrap();
        assert!(st.start_palette_resize(before.x, before.y + 20.0));
        assert!(st.resize_palette(before.x - 60.0));
        let wider = st.palette_bounds().unwrap();
        assert_eq!(wider.w, before.w + 60.0);
        assert_eq!(wider.right(), before.right());
        assert!(st.resize_palette(before.x + 100.0));
        assert_eq!(st.palette_bounds().unwrap().w, sp::DOCK_MIN_WIDTH);
    }

    #[test]
    fn the_close_button_hides_the_docked_palette() {
        let mut st = state();
        click(&mut st, Hit::Close);
        assert!(st.palette_bounds().is_none());
    }
}
