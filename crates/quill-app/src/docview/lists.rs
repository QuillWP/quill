//! Bulleted paragraphs as you type: Return carries the bullet on or ends
//! the list, Backspace takes it off, and "* " or "- " starts one. A bullet
//! is typed text (see `tools::bullet`), a mark and a tab that act as one.
//! In a Markdown file numbered items work the same way, and Return on an
//! empty paragraph in a quote takes it out of the quote.

use super::*;
use quill_core::CharProps;
use quill_core::tools::{end_list, list_mark_len, next_list_mark, numbered_mark_len, start_list};

impl DocView {
    /// The length of the list mark of paragraph `para`, if it has one.
    fn list_mark(&self, para: usize) -> Option<usize> {
        if self.in_markdown_source() {
            return None;
        }
        let text = &self.doc.main.paras[para].text;
        list_mark_len(text).or_else(|| {
            self.is_markdown_file()
                .then(|| numbered_mark_len(text))
                .flatten()
        })
    }

    /// Where the caret lands moving to `next`: never between a list mark
    /// and its tab.
    pub(super) fn past_list_mark(&self, next: Position, forward: bool) -> Position {
        match self.list_mark(next.para) {
            Some(len) if next.offset > 0 && next.offset < len => {
                Position::new(next.para, if forward { len } else { 0 })
            }
            _ => next,
        }
    }

    /// Whether Backspace, or Delete with `back` false, would hit the list
    /// mark at the caret, which goes as a whole.
    pub(super) fn on_list_mark(&self, back: bool) -> bool {
        let at = self.sel.focus;
        self.sel.is_empty()
            && self.list_mark(at.para).is_some_and(|len| {
                if back {
                    at.offset > 0 && at.offset <= len
                } else {
                    at.offset < len
                }
            })
    }

    /// Whether the caret is in a list paragraph with nothing after its mark.
    pub(super) fn on_empty_list_item(&self) -> bool {
        let at = self.sel.focus;
        self.sel.is_empty()
            && self.list_mark(at.para) == Some(self.doc.main.paras[at.para].len())
            && at.offset > 0
    }

    /// Take the list mark off the caret's paragraph, keeping its text. The
    /// caret stays where it was in that text.
    pub(super) fn end_list_here(&mut self) {
        let at = self.sel.focus;
        self.doc.begin("Bullet", self.sel);
        let len = end_list(&mut self.doc, MAIN, at.para);
        self.typing_chp = None;
        let offset = at.offset.saturating_sub(len);
        self.finish_edit(Selection::caret(Position::new(at.para, offset)));
    }

    /// The mark and tab to start the next paragraph with, when Return at
    /// `at` splits a list paragraph after its mark.
    pub(super) fn list_mark_to_carry(&self, at: Position) -> Option<(String, CharProps)> {
        let len = self.list_mark(at.para).filter(|len| at.offset >= *len)?;
        let p = &self.doc.main.paras[at.para];
        Some((next_list_mark(&p.text[..len]), p.chp_of_char(0)))
    }

    /// Whether the caret is in an empty paragraph of a quote, in a Markdown
    /// file.
    pub(super) fn on_empty_quote(&self) -> bool {
        let p = &self.doc.main.paras[self.sel.focus.para];
        self.sel.is_empty()
            && self.is_markdown_file()
            && !self.in_markdown_source()
            && p.is_empty()
            && !p.pap.in_table
            && p.pap.borders.left.style != quill_core::props::BorderStyle::None
    }

    /// Take the caret's paragraph out of one level of quote.
    pub(super) fn leave_quote_here(&mut self) {
        let para = self.sel.focus.para;
        self.doc.begin("Quote", self.sel);
        self.doc.format_paras(MAIN, para, para, |pap| {
            quill_formats::markdown::leave_quote(pap);
        });
        self.finish_edit(self.sel);
    }

    /// After a space: "* " or "- " at the start of a paragraph starts a
    /// list. It is an undo step of its own, so Command-Z leaves what was
    /// typed. `at` is where the space went.
    pub(super) fn auto_list(&mut self, at: Position) {
        if !crate::settings::with(|s| s.auto_lists) || self.in_markdown_source() || at.offset != 1 {
            return;
        }
        let mark = match self.doc.main.paras[at.para].text.chars().next() {
            Some('*') => '•',
            Some('-') => '-',
            _ => return,
        };
        let sel = self.sel;
        self.doc.begin("Bullet", sel);
        start_list(&mut self.doc, MAIN, at.para, 2, mark);
        self.typing_chp = None;
        let end = list_mark_len(&self.doc.main.paras[at.para].text).unwrap_or(0);
        self.finish_edit(Selection::caret(Position::new(at.para, end)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(text: &str) -> DocView {
        DocView::new(quill_core::text::from_plain_text(text), "Lists".into())
    }

    fn texts(v: &DocView) -> Vec<&str> {
        v.doc.main.paras.iter().map(|p| p.text.as_str()).collect()
    }

    #[test]
    fn typing_a_star_or_hyphen_starts_a_list_and_undo_leaves_the_typing() {
        let mut clip = Clipboard::without_system();
        let mut v = view("");
        v.type_text("*");
        v.type_text(" ");
        v.type_text("one");
        assert_eq!(texts(&v), ["•\tone"]);
        assert_eq!(v.doc.main.paras[0].pap.first, -360);

        let mut v = view("");
        v.type_text("-");
        v.type_text(" ");
        assert_eq!(texts(&v), ["-\t"]);
        v.exec(Cmd::Undo, false, &mut clip);
        assert_eq!(texts(&v), ["- "]);

        // Only at the start of a paragraph.
        let mut v = view("a");
        v.sel = Selection::caret(Position::new(0, 1));
        v.type_text("-");
        v.type_text(" ");
        assert_eq!(texts(&v), ["a- "]);
    }

    #[test]
    fn return_carries_the_bullet_and_ends_the_list_on_an_empty_one() {
        let mut clip = Clipboard::without_system();
        let mut v = view("");
        v.type_text("*");
        v.type_text(" ");
        v.type_text("one");
        v.exec(Cmd::NewParagraph, false, &mut clip);
        assert_eq!(texts(&v), ["•\tone", "•\t"]);
        assert_eq!(v.sel.focus, Position::new(1, 4));
        assert_eq!(v.doc.main.paras[1].pap.first, -360);
        v.exec(Cmd::NewParagraph, false, &mut clip);
        assert_eq!(texts(&v), ["•\tone", ""]);
        assert_eq!(v.sel.focus, Position::new(1, 0));
        assert_eq!(v.doc.main.paras[1].pap.first, 0);
        assert_eq!(v.doc.main.paras[1].pap.left, 0);
        v.type_text("after");
        assert_eq!(texts(&v), ["•\tone", "after"]);
    }

    #[test]
    fn return_before_the_bullet_is_an_ordinary_return() {
        let mut clip = Clipboard::without_system();
        let mut v = view("•\tone");
        v.exec(Cmd::NewParagraph, false, &mut clip);
        assert_eq!(texts(&v), ["", "•\tone"]);
    }

    #[test]
    fn backspace_and_delete_take_the_bullet_and_tab_together() {
        let mut clip = Clipboard::without_system();
        let mut v = view("");
        v.type_text("*");
        v.type_text(" ");
        v.type_text("one");
        v.exec(Cmd::Left, false, &mut clip);
        v.exec(Cmd::Left, false, &mut clip);
        v.exec(Cmd::Left, false, &mut clip);
        assert_eq!(v.sel.focus, Position::new(0, 4));
        v.exec(Cmd::Backspace, false, &mut clip);
        assert_eq!(texts(&v), ["one"]);
        assert_eq!(v.sel.focus, Position::new(0, 0));
        assert_eq!(v.doc.main.paras[0].pap.first, 0);
        v.exec(Cmd::Undo, false, &mut clip);
        assert_eq!(texts(&v), ["•\tone"]);
        v.sel = Selection::caret(Position::new(0, 0));
        v.exec(Cmd::DeleteForward, false, &mut clip);
        assert_eq!(texts(&v), ["one"]);
    }

    #[test]
    fn arrows_step_over_the_bullet_and_tab_as_one() {
        let mut clip = Clipboard::without_system();
        let mut v = view("•\tone");
        v.exec(Cmd::Right, false, &mut clip);
        assert_eq!(v.sel.focus, Position::new(0, 4));
        v.exec(Cmd::Left, false, &mut clip);
        assert_eq!(v.sel.focus, Position::new(0, 0));
    }

    #[test]
    fn the_setting_turns_typed_lists_off() {
        crate::settings::update(|s| s.auto_lists = false);
        let mut v = view("");
        v.type_text("*");
        v.type_text(" ");
        crate::settings::update(|s| s.auto_lists = true);
        assert_eq!(texts(&v), ["* "]);
    }

    #[test]
    fn return_on_empty_items_and_quotes_steps_out_of_a_markdown_file_s_blocks() {
        let mut clip = Clipboard::without_system();
        let (doc, _) = quill_formats::markdown::read("> Quoted\n>\n> 1. one\n", None);
        let mut v = DocView::new(doc, "Notes".into());
        v.path = Some("notes.md".into());
        let last = v.doc.main.paras.len() - 1;
        let end = v.doc.main.paras[last].len();
        v.sel = Selection::caret(Position::new(last, end));
        // Return carries the next number.
        v.exec(Cmd::NewParagraph, false, &mut clip);
        assert_eq!(v.doc.main.paras[last + 1].text, "2.\t");
        // On the empty item it ends the list, still in the quote.
        v.exec(Cmd::NewParagraph, false, &mut clip);
        assert_eq!(v.doc.main.paras.len(), last + 2);
        assert_eq!(v.doc.main.paras[last + 1].text, "");
        // Once more and it is out of the quote, back to normal text.
        v.exec(Cmd::NewParagraph, false, &mut clip);
        v.type_text("Plain");
        let md = quill_formats::markdown::write(&v.doc);
        assert!(md.contains("> 1. one"), "{md}");
        assert!(md.trim_end().ends_with("\n\nPlain"), "{md}");
        // Not in an ordinary document: there a numbered line is just text.
        let mut v = view("1.\tone");
        v.sel = Selection::caret(Position::new(0, 6));
        v.exec(Cmd::NewParagraph, false, &mut clip);
        assert_eq!(texts(&v), ["1.\tone", ""]);
    }
}
