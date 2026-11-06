//! One Review sidebar for tracked changes and editable comments.
use super::*;
use crate::canvas::round_rect;
use quill_core::{Position, StoryId};

pub(super) const BADGE_LANE: f32 = 44.0;
/// Keep the grab strip outside native scroll views, including when their
/// overlay scrollers are invisible but still receive mouse events.
pub(super) const DIVIDER_WIDTH: f32 = 8.0;
/// The title row and the All / Comments / Changes switch.
const HEADER: f32 = 68.0;
/// Previous, Next, Accept and Reject: their widths, and the height of a
/// row of them.
const BUTTON_WIDTHS: [f32; 4] = [70.0, 50.0, 54.0, 50.0];
const BUTTON_ROW: f32 = 28.0;
/// The "Comments" heading above the comment list.
const COMMENTS_HEADING: f32 = 34.0;
const PANEL: u32 = 0xF5F5F5;
const EDGE: u32 = 0xD0D0D0;
const TEXT: u32 = 0x202020;
const MUTED: u32 = 0x6E6E6E;
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
            return (HEADER + COMMENTS_HEADING).min(r.h);
        }
        (HEADER + buttons_height(r) + card_height(r) + COMMENTS_HEADING).min(r.h)
    }

    /// The change card: from under the buttons to the comments heading, or
    /// to the bottom of the pane when only changes show.
    fn card_bounds(&self, r: Rect) -> Rect {
        let top = r.y + HEADER + buttons_height(r);
        let bottom = if self.review_filter == Filter::Changes {
            r.bottom() - 12.0
        } else {
            r.y + self.review_text_top(r) - COMMENTS_HEADING
        };
        Rect::new(r.x + 12.0, top, r.w - 24.0, (bottom - top).max(0.0))
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
        if y < r.y + 34.0 {
            if close_box(r).inset(-4.0).contains(x, y) {
                self.close_pane();
                self.review_closed = true;
            }
            return Some(Effect::None);
        }
        if y < r.y + HEADER {
            self.focus_main();
            let tabs = tab_bounds(r);
            self.review_filter = match ((x - tabs.x) / (tabs.w / 3.0)).max(0.0) as usize {
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
            match review_buttons(r)
                .iter()
                .position(|b| b.inset(-3.0).contains(x, y))
            {
                Some(button @ (0 | 1)) => {
                    self.focus_main();
                    let index = self.revision_index();
                    let next = if self.main_view().change_here().is_none() {
                        0
                    } else if button == 0 {
                        index.saturating_sub(1)
                    } else {
                        (index + 1).min(changes.len() - 1)
                    };
                    let (a, b, _) = changes[next];
                    self.main_view_mut().select_found(a, b);
                }
                Some(button) => {
                    self.focus_main();
                    if self.main_view().change_here().is_none() {
                        let (a, b, _) = changes[self.revision_index()];
                        self.main_view_mut().select_found(a, b);
                    }
                    self.main_view_mut().resolve_change(button == 2);
                    self.sync_pane();
                }
                None => {}
            }
        }
        Some(Effect::None)
    }

    pub(super) fn draw_revisions(&mut self, c: &mut Canvas, label: &mut Label) {
        let Some(r) = self.revisions_bounds() else {
            return;
        };
        let font = quill_layout::fonts::UI_FONT;
        let comments = self.review_filter != Filter::Changes;
        let body_top = self.review_text_top(r);
        let pointer = self.pointer;
        let hover = |b: Rect| b.contains(pointer.0, pointer.1);
        let heading = Rect::new(r.x, r.y, r.w, if comments { body_top } else { r.h });
        let clip = c.set_clip(heading);
        c.fill(heading, PANEL);
        label.draw(c, font, 13.0, true, r.x + 14.0, r.y + 24.0, "Review", TEXT);
        let close = close_box(r);
        if hover(close) {
            round_rect(c, close, 5.0, 0xE2E2E2);
        }
        centered(c, label, close, 15.0, false, "×", MUTED);

        // A segmented switch, as in a Mac sidebar: one white segment for the
        // current filter on a gray track.
        let tabs = tab_bounds(r);
        round_rect(c, tabs, 6.0, 0xE3E3E3);
        let third = tabs.w / 3.0;
        let size = if label.width(font, 12.0, true, "Comments") > third - 10.0 {
            11.0
        } else {
            12.0
        };
        for (i, (text, filter)) in FILTERS.iter().enumerate() {
            let segment = Rect::new(tabs.x + i as f32 * third, tabs.y, third, tabs.h);
            let on = self.review_filter == *filter;
            if on {
                boxed(c, segment.inset(2.0), 5.0, 0xC8C8C8, color::WHITE);
            } else if i > 0 && self.review_filter != FILTERS[i - 1].1 {
                c.vline(segment.x, segment.y + 6.0, segment.bottom() - 6.0, 0xC4C4C4);
            }
            centered(c, label, segment, size, on, text, TEXT);
        }

        let changes = self.main_view().doc.changes_in(StoryId::Main);
        if self.review_filter != Filter::Comments {
            if changes.is_empty() {
                if self.review_filter == Filter::Changes {
                    let y = r.y + HEADER + 20.0;
                    label.draw(
                        c,
                        font,
                        12.0,
                        false,
                        r.x + 14.0,
                        y,
                        "No tracked changes",
                        MUTED,
                    );
                }
            } else {
                let here = self.main_view().change_here();
                let index = self.revision_index();
                let enabled = [
                    here.is_none() || index > 0,
                    here.is_none() || index + 1 < changes.len(),
                    true,
                    true,
                ];
                let names = ["‹ Previous", "Next ›", "Accept", "Reject"];
                for (i, b) in review_buttons(r).into_iter().enumerate() {
                    push_button(c, label, b, names[i], enabled[i], hover(b));
                }
                let shown = changes[index];
                if self.review_shown != Some(shown) {
                    self.review_detail_scroll = 0.0;
                    self.review_shown = Some(shown);
                }
                let (a, b, id) = shown;
                let rows = attribution(id);
                let mut card = self.card_bounds(r);
                let excerpt = self.change_excerpt(a, b);
                let lines = if excerpt.is_empty() {
                    Vec::new()
                } else {
                    wrap(label, &format!("\u{201C}{excerpt}\u{201D}"), card.w - 28.0)
                };
                if self.review_filter == Filter::Changes {
                    // With the pane to itself, the card is only as tall as its text.
                    let dated = rows.iter().filter(|(_, _, date)| !date.is_empty()).count();
                    let text = 42.0 + 22.0 * rows.len() as f32 + 16.0 * dated as f32;
                    card.h = card.h.min(text + 17.0 * lines.len() as f32 + 2.0);
                }
                boxed(c, card, 6.0, 0xD6D6D6, color::WHITE);
                // A stripe in the editor's color, as their changes show in the text.
                if let Some((_, author, _)) = rows.first() {
                    c.fill(
                        Rect::new(card.x + 1.0, card.y + 6.0, 3.0, (card.h - 12.0).max(0.0)),
                        author_color(author),
                    );
                }
                let detail_clip = c.set_clip(card.inset(1.0));
                let x = card.x + 14.0;
                let mut y = card.y + 20.0 - self.review_detail_scroll;
                let count = format!("Change {} of {}", index + 1, changes.len());
                label.draw(c, font, 11.0, false, x, y, &count, MUTED);
                y += 22.0;
                for (kind, author, date) in &rows {
                    let w = label.draw(c, font, 12.0, true, x, y, author, author_color(author));
                    let verb = kind.trim_end_matches(" by").to_lowercase();
                    label.draw(c, font, 12.0, false, x + w + 4.0, y, &verb, TEXT);
                    if !date.is_empty() {
                        y += 16.0;
                        label.draw(c, font, 11.0, false, x, y, &short_date(date), MUTED);
                    }
                    y += 22.0;
                }
                // The changed words, struck through when they are a deletion.
                if !lines.is_empty() {
                    let deleted = rows.iter().all(|(kind, _, _)| kind == "Deleted by");
                    for (i, line) in lines.iter().enumerate() {
                        let w = label.draw(c, font, 12.0, false, x, y, line, TEXT);
                        if deleted {
                            // Strike the words, not the quotation marks.
                            let open = if i == 0 {
                                label.width(font, 12.0, false, "\u{201C}")
                            } else {
                                0.0
                            };
                            let close = if i + 1 == lines.len() {
                                label.width(font, 12.0, false, "\u{201D}")
                            } else {
                                0.0
                            };
                            c.hline(x + open, x + w - close, y - 4.0, TEXT);
                        }
                        y += 17.0;
                    }
                }
                self.review_detail_max =
                    (y - 5.0 + self.review_detail_scroll - card.bottom()).max(0.0);
                self.review_detail_scroll = self.review_detail_scroll.min(self.review_detail_max);
                c.restore_clip(detail_clip);
            }
        }
        if comments {
            let y = r.y + body_top - 12.0;
            let w = label.draw(c, font, 12.0, true, r.x + 14.0, y, "Comments", TEXT);
            let count =
                quill_core::notes::annotation_ids(&self.main_view().doc.annotations.paras).len();
            if count > 0 {
                label.draw(
                    c,
                    font,
                    12.0,
                    false,
                    r.x + 20.0 + w,
                    y,
                    &count.to_string(),
                    MUTED,
                );
            }
            c.hline(r.x, r.right(), r.y + body_top - 1.0, EDGE);
        }
        c.restore_clip(clip);

        // The divider: part of the sidebar, with a grip in the middle that
        // darkens while it can be dragged.
        let strip = Rect::new(r.x - DIVIDER_WIDTH, r.y, DIVIDER_WIDTH, r.h);
        let active = self.review_resize.is_some() || self.review_divider_hit(pointer.0, pointer.1);
        c.fill(strip, if active { 0xE6E6E6 } else { PANEL });
        c.vline(strip.x, r.y, r.bottom(), EDGE);
        round_rect(
            c,
            Rect::new(
                strip.x + (DIVIDER_WIDTH - 3.0) / 2.0,
                r.y + (r.h - 36.0) / 2.0,
                3.0,
                36.0,
            ),
            1.5,
            if active { 0x8E8E8E } else { 0xC4C4C4 },
        );
        if comments && !self.has_comments() && !self.pane.as_ref().is_some_and(|p| p.focused) {
            let empty = Rect::new(r.x, r.y + body_top, r.w, (r.h - body_top).max(0.0));
            let clip = c.set_clip(empty);
            c.fill(empty, color::WHITE);
            label.draw(
                c,
                font,
                12.0,
                false,
                r.x + 14.0,
                empty.y + 24.0,
                "No comments",
                MUTED,
            );
            c.restore_clip(clip);
        }
    }

    /// The text a change covers, on one line and cut short, for the card.
    fn change_excerpt(&self, a: Position, b: Position) -> String {
        const MOST: usize = 80;
        let paras = &self.main_view().doc.main.paras;
        let mut out = String::new();
        let last = b.para.min(paras.len().saturating_sub(1));
        for (i, para) in paras.iter().enumerate().take(last + 1).skip(a.para) {
            let text = &para.text;
            let start = if i == a.para {
                a.offset.min(text.len())
            } else {
                0
            };
            let end = if i == b.para {
                b.offset.min(text.len())
            } else {
                text.len()
            };
            if i > a.para {
                out.push(' ');
            }
            // Leave out the marks that stand for pictures, fields and notes.
            out.extend(text.get(start..end).unwrap_or("").chars().filter(|&ch| {
                !ch.is_control() && !('\u{E000}'..='\u{F8FF}').contains(&ch) && ch != '\u{FFFC}'
            }));
            if out.chars().count() > MOST {
                break;
            }
        }
        let out = out.trim();
        if out.chars().count() > MOST {
            let cut: String = out.chars().take(MOST - 3).collect();
            format!("{}...", cut.trim_end())
        } else {
            out.to_string()
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
            // A pill in a pale wash of the editor's color.
            let font = quill_layout::fonts::UI_FONT;
            let w = label.width(font, 10.0, true, &badge.text);
            badge.rect.w = (w + 12.0).max(20.0);
            round_rect(c, badge.rect, badge.rect.h / 2.0, tint(badge.color, 0.85));
            label.draw(
                c,
                font,
                10.0,
                true,
                badge.rect.x + (badge.rect.w - w) / 2.0,
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

const FILTERS: [(&str, Filter); 3] = [
    ("All", Filter::All),
    ("Comments", Filter::Comments),
    ("Changes", Filter::Changes),
];

fn close_box(r: Rect) -> Rect {
    Rect::new(r.right() - 32.0, r.y + 7.0, 22.0, 22.0)
}

fn tab_bounds(r: Rect) -> Rect {
    Rect::new(r.x + 12.0, r.y + 36.0, r.w - 24.0, 22.0)
}

/// How many rows the change buttons take: one when all four fit, else
/// Previous and Next above Accept and Reject, else one button a row.
fn button_rows(r: Rect) -> usize {
    let room = r.w - 24.0;
    let [prev, next, accept, reject] = BUTTON_WIDTHS;
    if prev + next + accept + reject + 6.0 * 2.0 + 10.0 <= room {
        1
    } else if (prev + next).max(accept + reject) + 6.0 <= room {
        2
    } else {
        4
    }
}

fn buttons_height(r: Rect) -> f32 {
    button_rows(r) as f32 * BUTTON_ROW + 4.0
}

/// Previous, Next, Accept and Reject. On one row, Previous and Next sit
/// at the left and Accept and Reject at the right.
pub(super) fn review_buttons(r: Rect) -> [Rect; 4] {
    let top = r.y + HEADER;
    let (left, right) = (r.x + 12.0, r.right() - 12.0);
    let [prev, next, accept, reject] = BUTTON_WIDTHS;
    let button = |x: f32, row: usize, w: f32| {
        Rect::new(x, top + row as f32 * BUTTON_ROW, w, BUTTON_ROW - 6.0)
    };
    match button_rows(r) {
        1 => [
            button(left, 0, prev),
            button(left + prev + 6.0, 0, next),
            button(right - reject - 6.0 - accept, 0, accept),
            button(right - reject, 0, reject),
        ],
        2 => [
            button(left, 0, prev),
            button(left + prev + 6.0, 0, next),
            button(left, 1, accept),
            button(left + accept + 6.0, 1, reject),
        ],
        _ => [0, 1, 2, 3].map(|n| button(left, n, BUTTON_WIDTHS[n].min(right - left))),
    }
}

/// Room for the change card when comments share the pane.
fn card_height(r: Rect) -> f32 {
    (r.h * 0.22).clamp(96.0, 170.0)
}

fn author_color(author: &str) -> u32 {
    quill_core::revision::author_color(author).0
}

/// Mix `color` toward white; `k` of 1 is white.
fn tint(color: u32, k: f32) -> u32 {
    let mix = |shift: u32| {
        let v = ((color >> shift) & 0xFF) as f32;
        ((v + (255.0 - v) * k).round() as u32) << shift
    };
    mix(16) | mix(8) | mix(0)
}

/// "2026-10-23T09:00:00Z" as "2026-10-23 09:00", as the comments show it.
fn short_date(date: &str) -> String {
    let date = date.replace('T', " ");
    date.get(..16).unwrap_or(&date).to_string()
}

fn boxed(c: &mut Canvas, r: Rect, radius: f32, border: u32, fill: u32) {
    round_rect(c, r, radius, border);
    let hair = c.hair();
    round_rect(c, r.inset(hair), (radius - hair).max(1.0), fill);
}

fn centered(
    c: &mut Canvas,
    label: &mut Label,
    r: Rect,
    size: f32,
    bold: bool,
    text: &str,
    color: u32,
) {
    let font = quill_layout::fonts::UI_FONT;
    let w = label.width(font, size, bold, text);
    let baseline = r.y + (r.h + size * 0.7) / 2.0;
    label.draw(
        c,
        font,
        size,
        bold,
        r.x + (r.w - w) / 2.0,
        baseline,
        text,
        color,
    );
}

/// A bordered push button, as in the dialogs.
fn push_button(c: &mut Canvas, label: &mut Label, r: Rect, text: &str, enabled: bool, hover: bool) {
    let fill = if enabled && hover {
        0xEFEFEF
    } else {
        color::WHITE
    };
    boxed(c, r, 5.0, 0xC2C2C2, fill);
    centered(
        c,
        label,
        r,
        11.5,
        false,
        text,
        if enabled { TEXT } else { 0xA8A8A8 },
    );
}

/// Break `text` into lines no wider than `width`, between words where it
/// can and inside a word that is wider than a line on its own.
fn wrap(label: &mut Label, text: &str, width: f32) -> Vec<String> {
    let font = quill_layout::fonts::UI_FONT;
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if label.width(font, 12.0, false, &candidate) <= width {
            line = candidate;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        for ch in word.chars() {
            line.push(ch);
            if line.chars().count() > 1 && label.width(font, 12.0, false, &line) > width {
                line.pop();
                lines.push(std::mem::replace(&mut line, ch.to_string()));
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
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
        let buttons = review_buttons(r);
        let center = |b: Rect| (b.x + b.w / 2.0, b.y + b.h / 2.0);
        let (x, y) = center(buttons[0]);
        state.revision_click(x, y);
        assert_eq!(state.revision_index(), 0);
        let id = state.main_view().change_here().unwrap().2;
        assert_eq!(attribution(id)[0].1, "Ada");
        let (x, y) = center(buttons[1]);
        state.revision_click(x, y);
        assert_eq!(state.revision_index(), 1);
        let id = state.main_view().change_here().unwrap().2;
        assert_eq!(attribution(id)[0].1, "Grace");
        state.main_view_mut().resolve_all(true);
        assert!(state.revisions_bounds().is_none());
    }
}
