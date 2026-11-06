//! One Review sidebar for tracked changes and editable comments.
use super::*;
use quill_core::{Position, StoryId};

pub(super) const BADGE_LANE: f32 = 44.0;
/// Keep the grab strip outside native scroll views, including when their
/// overlay scrollers are invisible but still receive mouse events.
pub(super) const DIVIDER_WIDTH: f32 = 8.0;
const HEADER: f32 = 72.0;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Filter {
    All,
    Comments,
    Changes,
}
type Change = (Position, Position, u32);
pub(super) struct Badge {
    pub rect: Rect,
    pub tip: String,
    text: String,
    color: u32,
    changes: Vec<Change>,
}

impl WinState {
    fn has_comments(&self) -> bool {
        !quill_core::notes::annotation_ids(&self.main_view().doc.annotations.paras).is_empty()
            || !self.main_view().doc.annotation_info.is_empty()
    }

    pub(super) fn ensure_review(&mut self) {
        if self.pane.is_none()
            && !self.review_closed
            && (self.main_view().doc.has_changes() || self.has_comments())
        {
            self.open_pane(StoryId::Annotations);
            self.focus_main();
        }
    }

    pub(super) fn revisions_bounds(&self) -> Option<Rect> {
        if self
            .pane
            .as_ref()
            .is_some_and(|p| p.story != StoryId::Annotations)
            || (self.pane.is_none()
                && (self.review_closed
                    || (!self.main_view().doc.has_changes() && !self.has_comments())))
        {
            return None;
        }
        let (x, narrower) = self.side_toolbar();
        let area = Rect::new(
            x,
            self.text_top(),
            self.width - narrower - BAR,
            (self.height - BAR - self.text_top()).max(0.0),
        );
        Some(self.review_bounds(area))
    }

    pub(super) fn review_bounds(&self, area: Rect) -> Rect {
        let default = pane::bounds(StoryId::Annotations, area);
        let min_width = 200.0_f32.min(area.w * 0.5).max(0.0);
        let max_width = (area.w - 240.0).max(min_width);
        let width = self
            .review_width
            .unwrap_or(default.w)
            .clamp(min_width, max_width);
        Rect::new(area.right() - width, area.y, width, area.h)
    }

    pub(super) fn review_divider_hit(&self, x: f32, y: f32) -> bool {
        self.menu.is_none()
            && !self.dialog_has_keys()
            && self.revisions_bounds().is_some_and(|r| {
                y >= r.y && y < r.bottom() && x >= r.x - DIVIDER_WIDTH && x <= r.x + 4.0
            })
    }

    pub(super) fn resize_review(&mut self, x: f32) {
        if let (Some(grab), Some(r)) = (self.review_resize, self.revisions_bounds()) {
            let (left, narrower) = self.side_toolbar();
            let area = Rect::new(left, r.y, self.width - narrower - BAR, r.h);
            self.review_width = Some(r.right() - (x - grab));
            // Save a feasible preference; subsequent window resizing only clamps
            // its displayed width, so enlarging the window restores the preference.
            self.review_width = Some(self.review_bounds(area).w);
        }
    }

    /// Amount of the shared sidebar occupied by its heading and change card.
    pub(super) fn review_text_top(&self, r: Rect) -> f32 {
        if self.review_filter == Filter::Changes {
            return r.h;
        }
        if self.review_filter == Filter::Comments || !self.main_view().doc.has_changes() {
            return (HEADER + 28.0).min(r.h);
        }
        (HEADER + (r.h * 0.30).clamp(150.0, 230.0) + 28.0).min(r.h)
    }

    fn revision_index(&self) -> usize {
        let view = self.main_view();
        let changes = view.doc.changes_in(StoryId::Main);
        view.change_here()
            .and_then(|here| changes.iter().position(|c| *c == here))
            .unwrap_or(0)
    }

    pub(super) fn review_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let Some(r) = self.revisions_bounds() else {
            return false;
        };
        if !r.contains(x, y)
            || (self.review_filter != Filter::Changes && y >= r.y + self.review_text_top(r))
        {
            return false;
        }
        self.review_detail_scroll =
            (self.review_detail_scroll + dy / self.ui).clamp(0.0, self.review_detail_max);
        true
    }

    pub(super) fn revision_click(&mut self, x: f32, y: f32) -> Option<Effect> {
        if let Some(badge) = self.revision_badges.iter().find(|b| b.rect.contains(x, y)) {
            let changes = badge.changes.clone();
            let here = self.main_view().change_here();
            let index = here
                .and_then(|h| changes.iter().position(|c| *c == h))
                .map_or(0, |i| (i + 1) % changes.len());
            let (a, b, _) = changes[index];
            self.focus_main();
            if self
                .pane
                .as_ref()
                .is_some_and(|p| p.story != StoryId::Annotations)
            {
                self.close_pane();
            }
            self.review_closed = false;
            self.ensure_review();
            self.focus_main();
            if self.review_filter == Filter::Comments {
                self.review_filter = Filter::All;
            }
            self.main_view_mut().select_found(a, b);
            return Some(Effect::None);
        }
        let r = self.revisions_bounds()?;
        if !r.contains(x, y) {
            return None;
        }
        if y < r.y + 32.0 {
            if x > r.right() - 30.0 {
                self.close_pane();
                self.review_closed = true;
            }
            return Some(Effect::None);
        }
        if y < r.y + HEADER {
            self.focus_main();
            self.review_filter = match ((x - r.x) / (r.w / 3.0)) as usize {
                0 => Filter::All,
                1 => Filter::Comments,
                _ => Filter::Changes,
            };
            return Some(Effect::None);
        }
        let comments_top = r.y + self.review_text_top(r);
        if self.review_filter != Filter::Changes && y >= comments_top {
            // Keep the existing comment editor, including its undo and selection.
            return None;
        }
        if self.review_filter != Filter::Comments && self.main_view().doc.has_changes() {
            let changes = self.main_view().doc.changes_in(StoryId::Main);
            if y < r.y + HEADER + 32.0 {
                self.focus_main();
                let index = self.revision_index();
                let next = if self.main_view().change_here().is_none() {
                    0
                } else if x < r.x + r.w / 2.0 {
                    index.saturating_sub(1)
                } else {
                    (index + 1).min(changes.len() - 1)
                };
                let (a, b, _) = changes[next];
                self.main_view_mut().select_found(a, b);
            } else if y < r.y + HEADER + 64.0 {
                self.focus_main();
                if self.main_view().change_here().is_none() {
                    let (a, b, _) = changes[self.revision_index()];
                    self.main_view_mut().select_found(a, b);
                }
                self.main_view_mut().resolve_change(x < r.x + r.w / 2.0);
                self.sync_pane();
            }
        }
        Some(Effect::None)
    }

    pub(super) fn draw_revisions(&mut self, c: &mut Canvas, label: &mut Label) {
        let Some(r) = self.revisions_bounds() else {
            return;
        };
        let comments = self.review_filter != Filter::Changes;
        let body_top = self.review_text_top(r);
        let heading = Rect::new(r.x, r.y, r.w, if comments { body_top } else { r.h });
        let clip = c.set_clip(heading);
        c.fill(heading, 0xF5F5F5);
        let font = quill_layout::fonts::UI_FONT;
        label.draw(
            c,
            font,
            13.0,
            true,
            r.x + 12.0,
            r.y + 22.0,
            "Review",
            color::BLACK,
        );
        label.draw(
            c,
            font,
            16.0,
            false,
            r.right() - 24.0,
            r.y + 23.0,
            "×",
            0x555555,
        );
        for (i, (text, filter)) in [
            ("All", Filter::All),
            ("Comments", Filter::Comments),
            ("Changes", Filter::Changes),
        ]
        .iter()
        .enumerate()
        {
            review_button(
                c,
                label,
                Rect::new(
                    r.x + i as f32 * r.w / 3.0 + 4.0,
                    r.y + 36.0,
                    r.w / 3.0 - 8.0,
                    28.0,
                ),
                text,
                self.review_filter == *filter,
            );
        }
        let changes = self.main_view().doc.changes_in(StoryId::Main);
        if self.review_filter != Filter::Comments {
            if changes.is_empty() {
                if self.review_filter == Filter::Changes {
                    let mut y = r.y + HEADER + 24.0;
                    revision_line(c, label, r, &mut y, "No tracked changes.", false);
                }
            } else {
                let index = self.revision_index();
                let width = r.w / 2.0;
                for (i, text) in ["Previous", "Next", "Accept", "Reject"].iter().enumerate() {
                    review_button(
                        c,
                        label,
                        Rect::new(
                            r.x + (i % 2) as f32 * width + 4.0,
                            r.y + HEADER + (i / 2) as f32 * 32.0,
                            width - 8.0,
                            28.0,
                        ),
                        text,
                        false,
                    );
                }
                let shown = changes[index];
                if self.review_shown != Some(shown) {
                    self.review_detail_scroll = 0.0;
                    self.review_shown = Some(shown);
                }
                let detail_bottom = if comments {
                    r.y + body_top - 30.0
                } else {
                    r.bottom() - 8.0
                };
                let detail_top = r.y + HEADER + 68.0;
                let detail_clip = c.set_clip(Rect::new(
                    r.x,
                    detail_top,
                    r.w,
                    (detail_bottom - detail_top).max(0.0),
                ));
                let mut y = detail_top + 15.0 - self.review_detail_scroll;
                revision_line(
                    c,
                    label,
                    r,
                    &mut y,
                    &format!("Change {} of {}", index + 1, changes.len()),
                    true,
                );
                for (kind, author, date) in attribution(changes[index].2) {
                    revision_line(c, label, r, &mut y, &format!("{kind} {author}"), true);
                    if !date.is_empty() {
                        revision_line(c, label, r, &mut y, &date.replace('T', " "), false);
                    }
                    y += 8.0;
                }
                self.review_detail_max = (y + self.review_detail_scroll - detail_bottom).max(0.0);
                self.review_detail_scroll = self.review_detail_scroll.min(self.review_detail_max);
                c.restore_clip(detail_clip);
            }
        }
        if comments {
            let y = r.y + body_top - 8.0;
            label.draw(c, font, 12.0, true, r.x + 12.0, y, "Comments", color::BLACK);
            c.hline(r.x, r.right(), r.y + body_top - 1.0, 0xD0D0D0);
        }
        c.restore_clip(clip);
        c.vline(r.x, r.y, r.bottom(), 0xD0D0D0);
        let dragging =
            self.review_resize.is_some() || self.review_divider_hit(self.pointer.0, self.pointer.1);
        c.fill(
            Rect::new(r.x - DIVIDER_WIDTH, r.y, DIVIDER_WIDTH, r.h),
            if dragging { 0xDADADA } else { 0xF0F0F0 },
        );
        let center = r.x - DIVIDER_WIDTH / 2.0;
        c.vline(
            center,
            r.y + (r.h - 28.0) / 2.0,
            r.y + (r.h + 28.0) / 2.0,
            0xA0A0A0,
        );
        if comments && !self.has_comments() && !self.pane.as_ref().is_some_and(|p| p.focused) {
            let empty = Rect::new(
                r.x + 1.0,
                r.y + body_top,
                r.w - 1.0,
                (r.h - body_top).max(0.0),
            );
            let clip = c.set_clip(empty);
            c.fill(empty, 0xFFFFFF);
            label.draw(
                c,
                font,
                12.0,
                false,
                r.x + 12.0,
                empty.y + 24.0,
                "No comments.",
                0x555555,
            );
            c.restore_clip(clip);
        }
    }

    pub(super) fn draw_revision_badges(&mut self, c: &mut Canvas, label: &mut Label) {
        self.revision_badges.clear();
        let view = self.main_view();
        let ratio = self.document_zoom / self.ui;
        let x = view.area.right() * ratio + 3.0;
        let top = view.area.y * ratio;
        let bottom = view.area.bottom() * ratio;
        let anchors = view.revision_anchors();
        for (a, b, id, wy) in anchors {
            let y = (wy * ratio).max(top);
            if y + 18.0 > bottom {
                continue;
            }
            if let Some(badge) = self
                .revision_badges
                .iter_mut()
                .find(|badge| (badge.rect.y - y).abs() < 18.0)
            {
                badge.changes.push((a, b, id));
            } else {
                self.revision_badges.push(Badge {
                    rect: Rect::new(x, y, 38.0, 18.0),
                    tip: String::new(),
                    text: String::new(),
                    color: 0,
                    changes: vec![(a, b, id)],
                });
            }
        }
        for badge in &mut self.revision_badges {
            let mut names = Vec::new();
            let mut descriptions = Vec::new();
            for &(_, _, id) in &badge.changes {
                for (kind, author, _) in attribution(id) {
                    if !names.contains(&author) {
                        names.push(author.clone());
                    }
                    let description = format!("{kind} {author}");
                    if !descriptions.contains(&description) {
                        descriptions.push(description);
                    }
                }
            }
            let author = names
                .first()
                .map(String::as_str)
                .unwrap_or("Unknown author");
            badge.text = initials(author);
            if names.len() > 1 {
                badge.text.push('+');
            }
            badge.color = quill_core::revision::author_color(author).0;
            badge.tip = descriptions.join("\n");
            if badge.changes.len() > 1 {
                badge.tip.push_str("\nClick again to cycle changes");
            }
            c.fill(badge.rect, 0xEDEDED);
            c.frame(badge.rect, badge.color);
            label.draw(
                c,
                quill_layout::fonts::UI_FONT,
                10.0,
                true,
                badge.rect.x + 5.0,
                badge.rect.y + 13.0,
                &badge.text,
                badge.color,
            );
        }
    }
}

fn initials(author: &str) -> String {
    if author == "Unknown author" {
        return "?".into();
    }
    let words: Vec<_> = author.split_whitespace().collect();
    let mut result = String::new();
    if let Some(first) = words.first().and_then(|s| s.chars().next()) {
        result.extend(first.to_uppercase());
        if words.len() > 1
            && let Some(last) = words.last().and_then(|s| s.chars().next())
        {
            result.extend(last.to_uppercase());
        }
    }
    if result.is_empty() {
        result.push('?');
    }
    result
}

fn review_button(c: &mut Canvas, label: &mut Label, r: Rect, text: &str, selected: bool) {
    c.fill(r, if selected { 0xD7E8FA } else { 0xE5E5E5 });
    label.draw(
        c,
        quill_layout::fonts::UI_FONT,
        12.0,
        selected,
        r.x + 5.0,
        r.y + 19.0,
        text,
        color::BLACK,
    );
}

fn attribution(id: u32) -> Vec<(String, String, String)> {
    let Some(rev) = quill_core::revision::get(id) else {
        return Vec::new();
    };
    [
        ("Inserted by", rev.inserted.as_ref()),
        ("Deleted by", rev.deleted.as_ref()),
        (
            "Formatted by",
            rev.formatted.as_ref().map(|(stamp, _)| stamp),
        ),
    ]
    .into_iter()
    .filter_map(|(kind, stamp)| {
        stamp.map(|s| {
            (
                kind.into(),
                if s.author.trim().is_empty() {
                    "Unknown author".into()
                } else {
                    s.author.clone()
                },
                s.date.clone(),
            )
        })
    })
    .collect()
}

fn revision_line(c: &mut Canvas, label: &mut Label, r: Rect, y: &mut f32, text: &str, bold: bool) {
    let font = quill_layout::fonts::UI_FONT;
    let mut line = String::new();
    for ch in text.chars() {
        line.push(ch);
        if label.width(font, 12.0, bold, &line) > r.w - 30.0 || ch == '\n' {
            let last = line.pop().unwrap();
            label.draw(c, font, 12.0, bold, r.x + 12.0, *y, &line, color::BLACK);
            *y += 17.0;
            line.clear();
            if last != '\n' {
                line.push(last);
            }
        }
    }
    label.draw(c, font, 12.0, bold, r.x + 12.0, *y, &line, color::BLACK);
    *y += 17.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::revision::{Revision, Stamp, intern};
    #[test]
    fn badges_use_unicode_initials_and_handle_missing_authors() {
        assert_eq!(initials("Ada Lovelace"), "AL");
        assert_eq!(initials("  Émilie du Châtelet  "), "ÉC");
        assert_eq!(initials("Lucas"), "L");
        assert_eq!(initials(""), "?");
        assert_eq!(initials("Unknown author"), "?");
    }
    #[test]
    fn every_contributing_author_is_visible() {
        let id = intern(Revision {
            inserted: Some(Stamp::new("Ada", "2026-10-21")),
            deleted: Some(Stamp::new("Grace", "2026-10-22")),
            formatted: Some((Stamp::new("Linus", "2026-10-23"), Default::default())),
        });
        let rows = attribution(id);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].1, "Ada");
        assert_eq!(rows[1].1, "Grace");
        assert_eq!(rows[2].1, "Linus");
        assert_eq!(rows[2].2, "2026-10-23");
    }

    #[test]
    fn panel_navigation_selects_changes_and_resolving_all_removes_panel() {
        let mut view = DocView::new(Document::new(), "Review".into());
        view.doc.props.track_changes = true;
        view.doc.author = "Ada".into();
        view.insert_text("first ");
        view.doc.author = "Grace".into();
        view.insert_text("second");
        let mut state = WinState::new(view);
        state.width = 900.0;
        state.height = 700.0;
        let r = state.revisions_bounds().unwrap();
        state.revision_click(r.x + 10.0, r.y + HEADER + 10.0);
        assert_eq!(state.revision_index(), 0);
        let id = state.main_view().change_here().unwrap().2;
        assert_eq!(attribution(id)[0].1, "Ada");
        state.revision_click(r.x + r.w * 0.75, r.y + HEADER + 10.0);
        assert_eq!(state.revision_index(), 1);
        let id = state.main_view().change_here().unwrap().2;
        assert_eq!(attribution(id)[0].1, "Grace");
        state.main_view_mut().resolve_all(true);
        assert!(state.revisions_bounds().is_none());
    }
}
