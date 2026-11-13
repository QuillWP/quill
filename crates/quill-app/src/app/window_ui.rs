//! Window chrome geometry and rendering.

use super::*;
use quill_core::StoryId;

impl WinState {
    pub(super) fn open_context(&mut self, x: f32, y: f32, label: &mut Label, clip: &mut Clipboard) {
        let Some((m, actions)) = self.context_menu_at(x, y, clip) else {
            return;
        };
        let (cx, cy) = (x / self.ui, y / self.ui);
        let bounds = Rect::new(0.0, 0.0, self.width, self.height);
        let anchor = Rect::new(cx, 0.0, 0.0, 0.0);
        let mut open = menu::place(label, &m, anchor, bounds, false);
        open.rect.y = cy.min((self.height - open.rect.h - 2.0).max(0.0));
        self.menu = Some(MenuTrack {
            source: MenuSource::Context,
            menus: vec![m],
            actions,
            open,
            submenu: None,
            typed: (String::new(), None),
            opened: Instant::now(),
            held: false,
        });
    }

    /// The address of the link in the main text at (`x`, `y`), if any.
    pub(super) fn link_at_point(&self, x: f32, y: f32) -> Option<String> {
        if self.dialogs.iter().any(|(_, d)| !d.modeless) {
            return None;
        }
        let (cx, cy) = (x / self.ui, y / self.ui);
        if self
            .pane
            .as_ref()
            .is_some_and(|p| p.bounds.contains(cx, cy))
        {
            return None;
        }
        let (dx, dy) = self.document_point(x, y);
        if !self.view.area.contains(dx, dy) {
            return None;
        }
        let (pos, _) = self.view.hit(dx, dy);
        self.view.link_url_near(pos)
    }

    /// Right-click at (`x`, `y`): put the insertion point there unless it
    /// is in the selection, and build the menu for that place. None where
    /// the document has no menu to offer.
    pub(super) fn context_menu_at(
        &mut self,
        x: f32,
        y: f32,
        clip: &mut Clipboard,
    ) -> Option<(crate::menu::Menu, Vec<Action>)> {
        if self.dialogs.iter().any(|(_, d)| !d.modeless) {
            return None;
        }
        let (cx, cy) = (x / self.ui, y / self.ui);
        if let Some((bounds, story)) = self.pane.as_ref().map(|p| (p.bounds, p.story)) {
            if bounds.contains(cx, cy)
                && pane::hit(bounds, story, self.same_as_previous_allowed(), cx, cy)
                    == PaneHit::Text
            {
                self.focus_pane();
            } else if !bounds.contains(cx, cy) {
                self.focus_main();
            } else {
                return None;
            }
        } else if let Some((_, other)) = self.active_rects()
            && other.contains(cx, cy)
        {
            self.switch_pane();
        }
        let (dx, dy) = self.document_point(x, y);
        if !self.view.area.contains(dx, dy) {
            return None;
        }
        for (_, dialog) in &mut self.dialogs {
            if dialog.modeless {
                dialog.inactive = true;
            }
        }
        if !self.view.select_picture_at(dx, dy) {
            let (pos, upstream) = self.view.hit(dx, dy);
            self.view.context_at(pos, upstream);
        }
        let forms = clip.forms();
        let can_paste_text = forms.contains(&crate::paste_special::Form::Unformatted);
        let (m, actions) = app_menus::context_menu(&self.view, !forms.is_empty(), can_paste_text);
        Some((m, actions))
    }

    pub(super) fn toolbar_top(&self) -> f32 {
        if self.own_menu_bar {
            MENU_BAR_HEIGHT
        } else {
            0.0
        }
    }

    pub(super) fn formatting_toolbar_top(&self) -> f32 {
        self.toolbar_top()
            + if self.show_toolbar && self.toolbar_position() == 0 {
                toolbar::thickness(false)
            } else {
                0.0
            }
    }

    /// Preferences > Toolbar: 0 top, 1 left, 2 right.
    pub(super) fn toolbar_position(&self) -> u8 {
        crate::settings::with(|s| s.toolbar_position)
    }

    /// Where the toolbar is and how long it is, if it is showing.
    pub(super) fn toolbar_place(&self) -> Option<(toolbar::Place, f32)> {
        if !self.show_toolbar {
            return None;
        }
        let down = (self.height - BAR - self.text_top()).max(0.0);
        Some(match self.toolbar_position() {
            1 => (
                toolbar::Place {
                    x: 0.0,
                    y: self.text_top(),
                    vertical: true,
                },
                down,
            ),
            2 => (
                toolbar::Place {
                    x: self.width - BAR - toolbar::thickness(true),
                    y: self.text_top(),
                    vertical: true,
                },
                down,
            ),
            _ => (
                toolbar::Place {
                    x: 0.0,
                    y: self.toolbar_top(),
                    vertical: false,
                },
                (self.width - 128.0).max(0.0),
            ),
        })
    }

    /// How far the text area starts from the left, and how much narrower
    /// it is, for a toolbar docked at the side.
    pub(super) fn side_toolbar(&self) -> (f32, f32) {
        let (x, narrower) = self.toolbar_side();
        (x, narrower + Self::palette_dock_width())
    }

    /// The same for the toolbar alone, without the docked Style Sheets
    /// palette that sits between it and the text.
    pub(super) fn toolbar_side(&self) -> (f32, f32) {
        match (self.show_toolbar, self.toolbar_position()) {
            (true, 1) => (toolbar::thickness(true), toolbar::thickness(true)),
            (true, 2) => (0.0, toolbar::thickness(true)),
            _ => (0.0, 0.0),
        }
    }

    /// The toolbar button under a point, if the point is on the toolbar.
    pub(super) fn toolbar_hit(&self, x: f32, y: f32) -> Option<Option<usize>> {
        let (at, len) = self.toolbar_place()?;
        let strip = if at.vertical {
            Rect::new(at.x, at.y, toolbar::thickness(true), len)
        } else {
            Rect::new(at.x, at.y, len, toolbar::thickness(false))
        };
        strip
            .contains(x, y)
            .then(|| toolbar::hit(at, x, y, &self.tool_state()))
    }

    /// The toolbar button under a point, enabled or not, for its tooltip.
    pub(super) fn toolbar_button_at(&self, x: f32, y: f32) -> Option<usize> {
        let (at, _) = self.toolbar_place()?;
        toolbar::button_at(at, x, y)
    }

    /// The formatting or paragraph toolbar control under a point.
    pub(super) fn bar_hit(&self, x: f32, y: f32) -> Option<Hit> {
        if y < self.formatting_toolbar_top() {
            return None;
        }
        if self.show_formatting_toolbar && y < self.paragraph_toolbar_top() {
            return bars::formatting_toolbar_hit(self.formatting_toolbar_top(), x, y);
        }
        if self.show_paragraph_toolbar && y < self.ruler_top() {
            return bars::paragraph_toolbar_hit(self.paragraph_toolbar_top(), x, y);
        }
        None
    }

    /// Whether the header pane's Same As Previous can be clicked: its
    /// section is a later one with headers of its own, or the pane's text
    /// (not copied back yet while it has the keyboard) differs from the
    /// section before's.
    pub(super) fn same_as_previous_allowed(&self) -> bool {
        let Some(p) = &self.pane else {
            return false;
        };
        if p.story.header_index_checked().is_none() || p.section == 0 {
            return false;
        }
        let main = &self.main_view().doc;
        let pane_text = if p.focused {
            &self.view.doc.main
        } else {
            &p.other.doc.main
        };
        main.has_own_headers(p.section) || *pane_text != *main.header_story(p.section - 1, p.story)
    }

    pub(super) fn tool_state(&self) -> ToolState {
        ToolState {
            has_selection: !self.view.sel.is_empty(),
            can_undo: self.view.can_undo(),
            show_marks: self.view.show_marks,
        }
    }

    pub(super) fn paragraph_toolbar_top(&self) -> f32 {
        self.formatting_toolbar_top()
            + if self.show_formatting_toolbar {
                bars::formatting_toolbar_height()
            } else {
                0.0
            }
    }

    /// Where the ruler starts, or in outline view the outline bar.
    pub(super) fn ruler_top(&self) -> f32 {
        self.paragraph_toolbar_top()
            + if self.show_paragraph_toolbar {
                bars::paragraph_toolbar_height()
            } else {
                0.0
            }
    }

    pub(super) fn text_top(&self) -> f32 {
        if self.main_view().outline {
            return self.ruler_top() + crate::outline_bar::HEIGHT;
        }
        self.ruler_top() + if self.show_ruler { SCALE_HEIGHT } else { 0.0 }
    }

    pub(super) fn right_gutter(&self) -> f32 {
        if self.native_scrollbars { 0.0 } else { BAR }
    }

    pub(super) fn ruler_state(&self) -> RulerState<'_> {
        let v = &self.view;
        let z = self.view_zoom();
        // In a table the ruler measures from the current cell.
        let (text_left, text_width, columns) = match v.table_ruler() {
            Some((x, w, edges)) => (
                x * z / self.ui,
                w,
                edges.iter().map(|e| e * z / self.ui).collect(),
            ),
            None => (
                v.column_left_window() * z / self.ui,
                v.caret_column_twips() as f32 / 20.0,
                Vec::new(),
            ),
        };
        RulerState {
            pap: v.current_pap(),
            text_left,
            text_width,
            columns,
            inch: 72.0 * z / self.ui,
            tabs: self.dialog_tabs().map(|t| t.0),
            chosen: self.dialog_tabs().and_then(|t| t.1),
        }
    }

    pub(super) fn context<'a>(&'a self, env: &Env<'a>) -> MenuContext<'a> {
        MenuContext {
            view: &self.view,
            font_families: env.font_families,
            show_formatting_toolbar: self.show_formatting_toolbar,
            show_paragraph_toolbar: self.show_paragraph_toolbar,
            show_ruler: self.show_ruler,
            full_screen: self.full_screen,
            recent: env.recent,
            windows: env.windows,
        }
    }

    /// Draw the whole window.
    pub fn paint(
        &mut self,
        c: &mut Canvas,
        fonts: &mut Fonts,
        cache: &mut GlyphCache,
        env: &Env,
        focused: bool,
    ) {
        let (ww, wh) = c.size();
        c.dark = self.dark;
        let k = self.ui;
        let z = self.view_zoom();
        let (w, h) = (ww / k, wh / k);
        self.width = w;
        self.height = h;
        self.ensure_review();
        self.sync_pane();
        let top = self.text_top();
        let bottom = h - BAR;
        // The text area is in window points; everything else in interface
        // units.
        self.show_toolbar = crate::settings::with(|s| s.view_toolbar);
        let (x0, narrower) = self.side_toolbar();
        let gutter = self.right_gutter();
        let sidebar = self
            .pane
            .as_ref()
            .is_some_and(|p| p.story == StoryId::Annotations);
        let available = Rect::new(
            x0,
            top,
            w - if sidebar { BAR } else { gutter } - narrower,
            (bottom - top).max(0.0),
        );
        let background_scale = c.zoom(k);
        c.fill(
            Rect::new(0.0, top, w, (bottom - top).max(0.0)),
            color::WHITE,
        );
        c.set_scale(background_scale);
        let panel = self.pane.as_ref().map(|p| {
            if p.story == StoryId::Annotations {
                self.review_bounds(available)
            } else {
                pane::bounds(p.story, available)
            }
        });
        let split = panel.filter(|_| !sidebar).map_or(bottom, |r| r.y);
        let main_right = panel
            .filter(|_| sidebar)
            .map_or(available.right(), |r| r.x - revisions::DIVIDER_WIDTH);
        let badges = self.main_view().show_changes && self.main_view().doc.has_changes();
        let badge_lane = if badges { revisions::BADGE_LANE } else { 0.0 };
        let main_area = Rect::new(
            x0 * k / z,
            top * k / z,
            (main_right - x0 - badge_lane).max(0.0) * k / z,
            (split - top) * k / z,
        );
        let pane_text = if sidebar {
            self.review_text_top(panel.unwrap())
        } else {
            pane::TITLE
                + self
                    .pane
                    .as_ref()
                    .map_or(pane::ICONS, |p| pane::icons(p.story))
        };
        let panel = panel.unwrap_or_default();
        let pane_area = Rect::new(
            panel.x * k / z,
            (panel.y + pane_text) * k / z,
            (panel.w - BAR).max(0.0) * k / z,
            (panel.h - pane_text).max(0.0) * k / z,
        );
        self.main_view_mut().area = main_area;
        if let Some(p) = &mut self.pane {
            p.bounds = panel;
        }
        if let Some(v) = self.pane_view_mut() {
            v.area = pane_area;
        }
        // Without a split, the split box sits above the scroll bar.
        let split_box = !self.native_scrollbars && self.pane.is_none() && self.split.is_none();
        let box_h = if split_box { SPLIT_BOX } else { 0.0 };
        self.vscroll = Rect::new(
            if sidebar {
                w - narrower - BAR
            } else {
                main_right - if self.native_scrollbars { BAR } else { 0.0 }
            },
            top + box_h,
            BAR,
            split - top - box_h,
        );
        if self.pane.is_some() {
            self.vscroll_other = Rect::new(
                panel.right() - BAR,
                panel.y + pane_text,
                BAR,
                (panel.h - pane_text).max(0.0),
            );
        }
        let panes = self.active_rects();
        if let Some((a, o)) = panes {
            self.view.area = Rect::new(a.x * k / z, a.y * k / z, a.w * k / z, a.h * k / z);
            self.vscroll = Rect::new(w - BAR, a.y, BAR, a.h);
            self.vscroll_other = Rect::new(w - BAR, o.y, BAR, o.h);
        }
        let live =
            focused && self.menu.is_none() && !self.dialog_has_keys() && self.zoom_edit.is_none();
        // Hyphenation looks for its next place once a change is laid out.
        if let Some((Which::Hyphenation(st), d)) = self.dialogs.last_mut() {
            self.view.update_layout(fonts);
            // It reads the glyphs of paragraphs anywhere in the document.
            self.view.restore_all(fonts);
            crate::lang_dialogs::hyphenation_refresh(st, d, &mut self.view);
        }
        self.sync_comments(fonts, k / z);
        let resize_text_width = self.review_resize.and(self.review_resize_text_width);
        let mut views: Vec<(&mut DocView, bool)> = Vec::new();
        match &mut self.pane {
            Some(p) => {
                let (a, b) = (&mut self.view, &mut p.other);
                views.push((a, live));
                views.push((b, false));
            }
            None => views.push((&mut self.view, live)),
        }
        let document_scale = c.zoom(z);
        for (v, active) in views {
            // Move and clip the pane immediately during a drag, reusing its
            // existing wrapping. Reflow once at the final width on release.
            let width = v.area.w;
            if v.captions
                && let Some(frozen) = resize_text_width
            {
                v.area.w = frozen;
            }
            if v.captions {
                v.set_comment_scale(k / z);
            }
            v.update_layout(fonts);
            v.scroll_y = v.scroll_y.clamp(0.0, v.max_scroll());

            v.prepare(fonts);
            v.area.w = width;
            v.draw(c, fonts, cache, active);
        }
        // The other pane of a split window: the same view, scrolled and
        // placed as that pane.
        if let (Some((a, o)), Some(sp)) = (panes, &mut self.split) {
            let v = &mut self.view;
            sp.other_scroll = sp.other_scroll.clamp(0.0, v.max_scroll());
            std::mem::swap(&mut v.scroll_y, &mut sp.other_scroll);
            v.area = Rect::new(o.x * k / z, o.y * k / z, o.w * k / z, o.h * k / z);
            v.prepare(fonts);
            v.draw(c, fonts, cache, false);
            v.area = Rect::new(a.x * k / z, a.y * k / z, a.w * k / z, a.h * k / z);
            std::mem::swap(&mut v.scroll_y, &mut sp.other_scroll);
        }
        if let Some(d) = &self.frame_drag {
            let view = self.main_view();
            let clip = c.set_clip(view.area);
            c.frame(view.frame_target_rect(d.page, d.target), 0x0A84FF);
            c.restore_clip(clip);
        }
        c.set_scale(document_scale);

        let old = c.zoom(k);
        let max = self.main_view().max_scroll();
        let frac = if max > 0.0 {
            self.main_view().scroll_y / max
        } else {
            0.0
        };
        let pressed = self.scroll_press.map(|p| p.0);

        let mut label = Label { fonts, cache };

        if !self.native_scrollbars {
            chrome::draw_scroll(c, self.vscroll, frac, true, max > 0.0, pressed);
        }
        if split_box {
            // The split box: a grip to drag the window into two panes.
            let r = Rect::new(w - BAR, top, BAR, SPLIT_BOX);
            c.fill(r, chrome::TRACK);
            c.fill(Rect::new(r.x, r.y, c.hair(), r.h), chrome::EDGE);
            c.fill(Rect::new(r.x + 4.0, r.y + 2.0, r.w - 8.0, 1.0), 0xA0A0A0);
            c.fill(Rect::new(r.x + 4.0, r.y + 4.0, r.w - 8.0, 1.0), 0xA0A0A0);
        }
        if let (Some((a, o)), Some(sp)) = (panes, &self.split) {
            let of = if max > 0.0 {
                sp.other_scroll / max
            } else {
                0.0
            };
            if !self.native_scrollbars {
                chrome::draw_scroll(c, self.vscroll_other, of, true, max > 0.0, None);
            }
            // The bar between the panes, flat with hairline edges.
            let y = a.y.min(o.y) + if sp.upper_active { a.h } else { o.h };
            c.fill(Rect::new(0.0, y, w, DIVIDER), chrome::TRACK);
            c.fill(Rect::new(0.0, y, w, c.hair()), chrome::EDGE);
            c.fill(
                Rect::new(0.0, y + DIVIDER - c.hair(), w, c.hair()),
                chrome::EDGE,
            );
        }
        if let Some(dy) = self.split_drag {
            // The divider follows the pointer while it is dragged.
            c.fill(Rect::new(0.0, dy - 1.0, w, 2.0), color::GRAY_DARK);
        }
        let style = self.view.style_name();
        let status = Rect::new(0.0, h - BAR, w - gutter, BAR);
        let page = format!("Page {}", self.main_view().caret_page() + 1);
        let message = self.view.status.clone();

        if let Some(p) = &self.pane {
            let title = if p.story == StoryId::Annotations {
                "Comments".to_string()
            } else if p.story.header_index_checked().is_some()
                && self.main_view().doc.sections().len() > 1
            {
                format!("{} · Section {}", p.story.name(), p.section + 1)
            } else {
                p.story.name().to_string()
            };
            let same = self.same_as_previous_allowed();
            if p.story != StoryId::Annotations {
                pane::draw(
                    c,
                    &mut label,
                    p.bounds,
                    &title,
                    p.focused && focused,
                    p.story,
                    same,
                );
            }
            if !self.native_scrollbars {
                let v = self.pane_view().unwrap();
                let max = v.max_scroll();
                let frac = if max > 0.0 { v.scroll_y / max } else { 0.0 };
                chrome::draw_scroll(
                    c,
                    self.vscroll_other,
                    frac,
                    true,
                    max > 0.0,
                    if p.focused { pressed } else { None },
                );
            }
        }
        self.draw_revisions(c, &mut label);
        self.draw_revision_badges(c, &mut label);
        self.draw_comment_links(c, z / k);
        let hbar = chrome::draw_status(c, &mut label, status, &page, &style, message.as_deref());
        if !self.native_scrollbars {
            let v = self.main_view();
            let max = v.max_scroll_x();
            chrome::draw_scroll(
                c,
                hbar,
                if max > 0.0 { v.scroll_x / max } else { 0.0 },
                false,
                max > 0.0,
                self.horizontal_press.map(|p| p.0),
            );
            chrome::draw_grow_box(c, Rect::new(w - BAR, h - BAR, BAR, BAR));
        }

        if let Some((at, len)) = self.toolbar_place() {
            let ts = self.tool_state();
            let clip = c.set_clip(if at.vertical {
                Rect::new(at.x, at.y, toolbar::thickness(true), len)
            } else {
                Rect::new(at.x, at.y, len, toolbar::thickness(false))
            });
            toolbar::draw(
                c,
                &mut label,
                at,
                len,
                &ts,
                self.toolbar_hover,
                self.toolbar_press,
            );
            c.restore_clip(clip);
        }
        self.draw_palette(c, &mut label);
        if self.show_formatting_toolbar {
            let chp = self.view.current_chp();
            let font = self.view.doc.font_name(chp.font).to_string();
            let rs = FormattingToolbarState { font: &font, chp };
            bars::draw_formatting_toolbar(
                c,
                &mut label,
                self.formatting_toolbar_top(),
                w,
                &rs,
                self.bar_hover,
            );
        }
        if self.show_paragraph_toolbar {
            let v = &self.view;
            let ps = ParagraphToolbarState {
                style: v.style_name().into(),
                pap: v.current_pap(),
                tab_type: self.tab_type,
                columns: v.current_section().columns,
            };
            bars::draw_paragraph_toolbar(
                c,
                &mut label,
                self.paragraph_toolbar_top(),
                w,
                &ps,
                self.bar_hover,
            );
        }
        let outline = self.main_view().outline;
        if outline {
            let v = self.main_view();
            crate::outline_bar::draw(
                c,
                &mut label,
                self.ruler_top(),
                w,
                v.first_line_only,
                v.show_formatting,
            );
        } else if self.show_ruler {
            let rs = self.ruler_state();
            bars::draw_ruler(c, &mut label, self.ruler_top(), w, &rs);
        }
        self.draw_zoom(c, &mut label);
        if self.own_menu_bar {
            let open_title = match self.menu.as_ref().map(|t| t.source) {
                Some(MenuSource::Bar(i)) => Some(i),
                _ => None,
            };
            let titles = self.bar_menus(env);
            menu::draw_bar(c, &mut label, &titles, w, open_title);
        }
        if let Some(t) = &self.menu {
            let draw = if t.source == MenuSource::Context {
                menu::draw_context_menu
            } else {
                menu::draw_menu
            };
            draw(c, &mut label, t.menu(), &t.open);
            if let Some((parent, open)) = &t.submenu
                && let Some(submenu) = t.menu().items[*parent].submenu.as_ref()
            {
                draw(c, &mut label, submenu, open);
            }
        }
        self.draw_tool_palette(c, &mut label);
        let below_ruler = self.text_top() + 8.0;
        for (which, d) in &mut self.dialogs {
            #[cfg(target_os = "macos")]
            if self.native_dialogs && crate::mac_dialog::supported(d) {
                continue;
            }
            #[cfg(windows)]
            if self.native_dialogs && crate::win_dialog::supported(d) {
                continue;
            }
            if d.origin == (0.0, 0.0) {
                d.place(w, h, below_ruler);
            }
            d.draw(c, &mut label, &mut |c, l, id, r| {
                format_dialogs::draw_custom(which, id, r, c, l)
            });
        }
        if let Some(t) = &self.tip
            && t.shown
            && self.menu.is_none()
            && self.dialogs.is_empty()
        {
            chrome::draw_tip(c, &mut label, &t.text, t.x, t.y, w, h);
        }
        c.set_scale(old);
    }

    /// The bar's menus as they would open now.
    pub(super) fn bar_menus(&self, env: &Env) -> Vec<Menu> {
        match &self.menu {
            Some(t) if matches!(t.source, MenuSource::Bar(_)) => t.menus.clone(),
            _ => app_menus::build(&self.context(env)).0,
        }
    }

    pub(super) fn title_at(&self, label: &mut Label, env: &Env, x: f32, y: f32) -> Option<usize> {
        if !self.own_menu_bar || y >= MENU_BAR_HEIGHT {
            return None;
        }
        let titles = menu::title_rects(label, &self.bar_menus(env), self.width);
        titles.iter().position(|r| r.contains(x, y))
    }

    pub(super) fn open_bar_menu(&mut self, i: usize, label: &mut Label, env: &Env) {
        let (menus, actions) = app_menus::build(&self.context(env));
        let titles = menu::title_rects(label, &menus, self.width);
        let bounds = Rect::new(0.0, 0.0, self.width, self.height);
        let Some(anchor) = titles.get(i).copied() else {
            return;
        };
        let mut open = menu::place(label, &menus[i], anchor, bounds, true);
        open.menu = i;
        self.menu = Some(MenuTrack {
            source: MenuSource::Bar(i),
            menus,
            actions,
            open,
            submenu: None,
            typed: (String::new(), None),
            opened: Instant::now(),
            held: true,
        });
    }

    /// Drop the Toolbar menu from button `i`: below it on a toolbar across
    /// the top, beside it on one down the side.
    pub(super) fn open_toolbar_menu(&mut self, i: usize, label: &mut Label) {
        let Some((at, _)) = self.toolbar_place() else {
            return;
        };
        let Some(anchor) = toolbar::button_rect(at, i) else {
            return;
        };
        let (m, actions) = app_menus::toolbar_menu(crate::settings::with(|s| s.toolbar_position));
        let bounds = Rect::new(0.0, 0.0, self.width, self.height);
        let open = if at.vertical {
            menu::place_side(label, &m, anchor, bounds)
        } else {
            menu::place(label, &m, anchor, bounds, true)
        };
        self.menu = Some(MenuTrack {
            source: MenuSource::Context,
            menus: vec![m],
            actions,
            open,
            submenu: None,
            typed: (String::new(), None),
            opened: Instant::now(),
            held: true,
        });
    }

    /// A key while a menu of Quill's own is open: the arrows move through
    /// it and in and out of a submenu, Return chooses, and typing jumps to
    /// the item that starts with what was typed.
    pub(super) fn menu_key(
        &mut self,
        key: &Key,
        text: Option<&str>,
        label: &mut Label,
        clip: &mut Clipboard,
    ) -> Effect {
        let bounds = Rect::new(0.0, 0.0, self.width, self.height);
        let Some(t) = &mut self.menu else {
            return Effect::None;
        };
        let menu = t.index();
        let in_child = t.submenu.as_ref().is_some_and(|(_, c)| c.hot.is_some());
        match key {
            Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp) => {
                let down = *key == Key::Named(NamedKey::ArrowDown);
                if let (true, Some((parent, child))) = (in_child, &mut t.submenu) {
                    if let Some(m) = t.menus[menu].items[*parent].submenu.as_ref() {
                        menu::step(m, child, down);
                    }
                } else {
                    t.submenu = None;
                    menu::step(&t.menus[menu], &mut t.open, down);
                }
            }
            Key::Named(NamedKey::ArrowRight) if !in_child => {
                let Some(i) = t.open.hot else {
                    return Effect::None;
                };
                let m = &t.menus[menu];
                if let (Some(child), Some(parent)) =
                    (m.items[i].submenu.as_ref(), menu::item_rect(m, &t.open, i))
                {
                    let mut open = menu::place_side(label, child, parent, bounds);
                    menu::step(child, &mut open, true);
                    t.submenu = Some((i, open));
                }
            }
            Key::Named(NamedKey::ArrowLeft) if in_child => t.submenu = None,
            Key::Named(NamedKey::Enter) => {
                let m = t.menu();
                let chosen = match &t.submenu {
                    Some((parent, child)) if in_child => m.items[*parent]
                        .submenu
                        .as_ref()
                        .zip(child.hot)
                        .map(|(sub, i)| sub.items[i].id),
                    _ => t.open.hot.map(|i| m.items[i].id),
                };
                let Some(id) = chosen else {
                    return Effect::None;
                };
                let action = t.actions.get(id as usize).cloned().unwrap_or(Action::None);
                self.menu = None;
                return self.act(action, clip);
            }
            _ => {
                // On the Mac typed letters come as the key, with no text.
                let typed = match key {
                    Key::Character(c) => Some(c.as_str()),
                    Key::Named(NamedKey::Space) => Some(" "),
                    _ => text,
                };
                let typed = typed.filter(|s| s.chars().all(|c| !c.is_control()));
                let Some(typed) = typed.filter(|s| !s.is_empty()) else {
                    return Effect::None;
                };
                let fresh = t
                    .typed
                    .1
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(1));
                if !fresh {
                    t.typed.0.clear();
                }
                t.typed.0.push_str(typed);
                t.typed.1 = Some(Instant::now());
                let found = t.typed.0.clone();
                if let (true, Some((parent, child))) = (in_child, &mut t.submenu) {
                    if let Some(m) = t.menus[menu].items[*parent].submenu.as_ref() {
                        menu::jump(m, child, &found);
                    }
                } else if menu::jump(&t.menus[menu], &mut t.open, &found) {
                    t.submenu = None;
                }
            }
        }
        Effect::None
    }

    pub(super) fn open_popup(&mut self, hit: Hit, label: &mut Label, env: &Env) {
        if hit == Hit::ColorPopup {
            // Font Color is the palette, not a list of names.
            let anchor = bars::popup_rect(
                hit,
                self.formatting_toolbar_top(),
                self.paragraph_toolbar_top(),
            );
            self.open_tool_palette(anchor);
            return;
        }
        let cx = self.context(env);
        let (m, actions) = match hit {
            Hit::FontPopup => app_menus::font_popup(&cx),
            Hit::SizePopup => app_menus::size_popup(&cx),
            _ => app_menus::style_popup(&cx),
        };
        let anchor = bars::popup_rect(
            hit,
            self.formatting_toolbar_top(),
            self.paragraph_toolbar_top(),
        );
        let bounds = Rect::new(0.0, 0.0, self.width, self.height);
        let open = menu::place(label, &m, anchor, bounds, true);
        self.menu = Some(MenuTrack {
            source: MenuSource::Popup(hit),
            menus: vec![m],
            actions,
            open,
            submenu: None,
            typed: (String::new(), None),
            opened: Instant::now(),
            held: true,
        });
    }
}
