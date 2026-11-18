//! Editable Font and Size fields in the formatting toolbar.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Font,
    Size,
}

pub(super) struct Edit {
    pub kind: Kind,
    pub text: String,
    pub anchor: usize,
    pub caret: usize,
    fonts: Vec<String>,
}

fn previous_boundary(text: &str, at: usize) -> usize {
    text[..at].char_indices().next_back().map_or(0, |(i, _)| i)
}

fn next_boundary(text: &str, at: usize) -> usize {
    text[at..]
        .char_indices()
        .nth(1)
        .map_or(text.len(), |(i, _)| at + i)
}

impl Edit {
    fn replace(&mut self, value: &str, complete: bool) {
        let (a, b) = (self.anchor.min(self.caret), self.anchor.max(self.caret));
        let value: String = value
            .chars()
            .filter(|c| match self.kind {
                Kind::Font => !c.is_control(),
                Kind::Size => c.is_ascii_digit() || matches!(c, '.' | ' ' | 'p' | 't'),
            })
            .collect();
        let limit = if self.kind == Kind::Font { 128 } else { 12 };
        if self.text.len() - (b - a) + value.len() > limit {
            return;
        }
        self.text.replace_range(a..b, &value);
        self.caret = a + value.len();
        self.anchor = self.caret;
        if complete && self.kind == Kind::Font {
            self.complete_font();
        }
    }

    fn complete_font(&mut self) {
        let prefix = self.text[..self.caret].trim_end().to_string();
        if prefix.is_empty() {
            return;
        }
        let Some(found) = self
            .fonts
            .iter()
            .find(|name| {
                name.get(..prefix.len())
                    .is_some_and(|p| p.eq_ignore_ascii_case(&prefix))
            })
            .cloned()
        else {
            return;
        };
        let typed = prefix.len();
        self.text.clone_from(&found);
        if found.eq_ignore_ascii_case(&prefix) {
            self.anchor = found.len();
            self.caret = found.len();
        } else {
            self.anchor = typed;
            self.caret = found.len();
        }
    }
}

impl WinState {
    pub(super) fn begin_format_edit(&mut self, kind: Kind, fonts: &[FontFamilyInfo]) {
        let text = match kind {
            Kind::Font => {
                let chp = self.view.current_chp();
                self.view.doc.font_name(chp.font).to_string()
            }
            Kind::Size => crate::bars::format_size(self.view.current_chp().size),
        };
        let end = text.len();
        self.format_edit = Some(Edit {
            kind,
            text,
            anchor: 0,
            caret: end,
            fonts: fonts.iter().map(|family| family.name.clone()).collect(),
        });
        for (_, dialog) in &mut self.dialogs {
            if dialog.modeless {
                dialog.inactive = true;
            }
        }
    }

    pub(super) fn commit_format_edit(&mut self) -> bool {
        let Some(edit) = &self.format_edit else {
            return true;
        };
        match edit.kind {
            Kind::Font => {
                let Some(name) = edit
                    .fonts
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(edit.text.trim()))
                    .cloned()
                else {
                    self.view.status = Some("Choose an installed font family.".into());
                    return false;
                };
                let id = self.view.doc.font_id(&name);
                self.view.format_chars("Formatting", move |c| c.font = id);
            }
            Kind::Size => {
                let Some(size) = crate::format_dialogs::parse_font_size(&edit.text) else {
                    self.view.status = Some("Enter a font size from 1 to 1638 points.".into());
                    return false;
                };
                self.view.set_size(size);
            }
        }
        self.format_edit = None;
        self.view.status = None;
        true
    }

    pub(super) fn format_edit_text(&mut self, text: &str) {
        if let Some(edit) = &mut self.format_edit {
            edit.replace(text, true);
        }
    }

    pub(super) fn format_edit_selection(&self) -> Option<String> {
        let edit = self.format_edit.as_ref()?;
        let (a, b) = (edit.anchor.min(edit.caret), edit.anchor.max(edit.caret));
        (a != b).then(|| edit.text[a..b].to_string())
    }

    pub(super) fn format_edit_key(&mut self, key: DKey, text: Option<&str>) {
        match key {
            DKey::Return | DKey::Tab { .. } => {
                self.commit_format_edit();
            }
            DKey::Escape => self.format_edit = None,
            _ => {
                let Some(edit) = &mut self.format_edit else {
                    return;
                };
                match key {
                    DKey::SelectAll => {
                        edit.anchor = 0;
                        edit.caret = edit.text.len();
                    }
                    DKey::Backspace => {
                        if edit.anchor == edit.caret {
                            edit.anchor = previous_boundary(&edit.text, edit.caret);
                        }
                        edit.replace("", false);
                    }
                    DKey::Delete => {
                        if edit.anchor == edit.caret {
                            edit.anchor = next_boundary(&edit.text, edit.caret);
                        }
                        edit.replace("", false);
                    }
                    DKey::Left => {
                        edit.caret = if edit.anchor != edit.caret {
                            edit.anchor.min(edit.caret)
                        } else {
                            previous_boundary(&edit.text, edit.caret)
                        };
                        edit.anchor = edit.caret;
                    }
                    DKey::Right => {
                        edit.caret = if edit.anchor != edit.caret {
                            edit.anchor.max(edit.caret)
                        } else {
                            next_boundary(&edit.text, edit.caret)
                        };
                        edit.anchor = edit.caret;
                    }
                    DKey::Char => {
                        if let Some(text) = text {
                            edit.replace(text, true);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_layout::FontVariant;

    fn family(name: &str) -> FontFamilyInfo {
        FontFamilyInfo {
            name: name.into(),
            variants: vec![FontVariant {
                bold: false,
                italic: false,
            }],
            draws_name: true,
        }
    }

    #[test]
    fn font_input_completes_a_family_and_size_input_accepts_half_points() {
        let mut state = WinState::new(DocView::new(Document::new(), "Edit fields".into()));
        state.begin_format_edit(Kind::Font, &[family("Georgia"), family("Gill Sans")]);
        state.format_edit_text("Gi");
        let edit = state.format_edit.as_ref().unwrap();
        assert_eq!(&edit.text, "Gill Sans");
        assert_eq!(&edit.text[edit.anchor..edit.caret], "ll Sans");
        state.format_edit_key(DKey::Backspace, None);
        assert_eq!(state.format_edit.as_ref().unwrap().text, "Gi");
        state.format_edit_text("ll");
        assert!(state.commit_format_edit());
        assert_eq!(
            state.view.doc.font_name(state.view.current_chp().font),
            "Gill Sans"
        );

        state.begin_format_edit(Kind::Size, &[]);
        state.format_edit_text("10.5 pt");
        assert!(state.commit_format_edit());
        assert_eq!(state.view.current_chp().size, 21);
    }

    #[test]
    fn invalid_font_and_size_leave_the_field_open() {
        let mut state = WinState::new(DocView::new(Document::new(), "Edit fields".into()));
        state.begin_format_edit(Kind::Font, &[family("Georgia")]);
        state.format_edit_text("Missing");
        assert!(!state.commit_format_edit());
        assert!(state.format_edit.is_some());
        state.format_edit = None;

        state.begin_format_edit(Kind::Size, &[]);
        state.format_edit_text("0");
        assert!(!state.commit_format_edit());
        assert!(state.format_edit.is_some());
    }
}
