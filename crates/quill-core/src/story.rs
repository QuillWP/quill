//! A story is a run of paragraphs: the main text, a header, a footnote.
//! Positions and selections point into a story.

use crate::para::Paragraph;

/// A place between two characters. `offset` is a byte offset into the
/// paragraph's text and always lies on a char boundary. `offset ==
/// text.len()` is just before the paragraph mark.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub para: usize,
    pub offset: usize,
}

impl Position {
    pub const fn new(para: usize, offset: usize) -> Self {
        Position { para, offset }
    }
}

/// A selection: where it started (`anchor`) and where it ends (`focus`,
/// the end that moves when extending). Empty when they are equal, which is
/// a plain insertion point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Selection {
    pub anchor: Position,
    pub focus: Position,
}

impl Selection {
    pub const fn caret(at: Position) -> Self {
        Selection {
            anchor: at,
            focus: at,
        }
    }

    pub const fn new(anchor: Position, focus: Position) -> Self {
        Selection { anchor, focus }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }

    pub fn start(&self) -> Position {
        self.anchor.min(self.focus)
    }

    pub fn end(&self) -> Position {
        self.anchor.max(self.focus)
    }

    pub fn range(&self) -> (Position, Position) {
        (self.start(), self.end())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharClass {
    Word,
    Space,
    Punct,
}

fn class(c: char) -> CharClass {
    if c.is_alphanumeric() || c == '\'' || c == '’' || c == '_' {
        CharClass::Word
    } else if c.is_whitespace() {
        CharClass::Space
    } else {
        CharClass::Punct
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Story {
    pub paras: Vec<Paragraph>,
}

impl Story {
    /// A story always has at least one paragraph; the last one's mark can
    /// never be deleted.
    pub fn new(first: Paragraph) -> Self {
        Story { paras: vec![first] }
    }

    pub fn para(&self, i: usize) -> &Paragraph {
        &self.paras[i]
    }

    pub fn start(&self) -> Position {
        Position::new(0, 0)
    }

    pub fn end(&self) -> Position {
        let last = self.paras.len() - 1;
        Position::new(last, self.paras[last].len())
    }

    /// Clamp a position so it is valid in this story.
    pub fn clamp(&self, mut pos: Position) -> Position {
        if pos.para >= self.paras.len() {
            return self.end();
        }
        let text = &self.paras[pos.para].text;
        pos.offset = pos.offset.min(text.len());
        while !text.is_char_boundary(pos.offset) {
            pos.offset -= 1;
        }
        pos
    }

    /// The position one character to the right, crossing paragraph marks.
    pub fn next_char(&self, pos: Position) -> Position {
        let text = &self.paras[pos.para].text;
        match text[pos.offset..].chars().next() {
            Some(c) => Position::new(pos.para, pos.offset + c.len_utf8()),
            None if pos.para + 1 < self.paras.len() => Position::new(pos.para + 1, 0),
            None => pos,
        }
    }

    /// The position one character to the left, crossing paragraph marks.
    pub fn prev_char(&self, pos: Position) -> Position {
        let text = &self.paras[pos.para].text;
        match text[..pos.offset].chars().next_back() {
            Some(c) => Position::new(pos.para, pos.offset - c.len_utf8()),
            None if pos.para > 0 => Position::new(pos.para - 1, self.paras[pos.para - 1].len()),
            None => pos,
        }
    }

    /// Start of the next word, the original's way: skip the rest of this word (or
    /// run of punctuation), then any spaces. Stops at paragraph ends.
    pub fn next_word(&self, pos: Position) -> Position {
        let text = &self.paras[pos.para].text;
        if pos.offset >= text.len() {
            return self.next_char(pos);
        }
        let mut chars = text[pos.offset..].char_indices().peekable();
        let first = class(chars.peek().unwrap().1);
        let mut end = text.len();
        let mut in_space = first == CharClass::Space;
        for (i, c) in chars {
            let k = class(c);
            if in_space {
                if k != CharClass::Space {
                    end = pos.offset + i;
                    break;
                }
            } else if k == CharClass::Space {
                in_space = true;
            } else if k != first {
                end = pos.offset + i;
                break;
            }
        }
        Position::new(pos.para, end)
    }

    /// Start of the previous word (skipping spaces first).
    pub fn prev_word(&self, pos: Position) -> Position {
        if pos.offset == 0 {
            return self.prev_char(pos);
        }
        let text = &self.paras[pos.para].text;
        let mut iter = text[..pos.offset].char_indices().rev().peekable();
        while let Some(&(_, c)) = iter.peek() {
            if class(c) != CharClass::Space {
                break;
            }
            iter.next();
        }
        let Some(&(_, c)) = iter.peek() else {
            return Position::new(pos.para, 0);
        };
        let k = class(c);
        let mut start = 0;
        for (i, c) in iter {
            if class(c) != k {
                start = i + c.len_utf8();
                break;
            }
        }
        Position::new(pos.para, start)
    }

    /// The word around `pos` as Word's double-click selects it: the word
    /// and its trailing spaces. On a paragraph end, the mark alone.
    pub fn word_at(&self, pos: Position) -> (Position, Position) {
        let text = &self.paras[pos.para].text;
        if text.is_empty() || pos.offset >= text.len() {
            if pos.offset > 0 {
                let start = self.prev_word(pos);
                return (start, Position::new(pos.para, text.len()));
            }
            return (pos, self.next_char(pos));
        }
        let c = text[pos.offset..].chars().next().unwrap();
        let k = class(c);
        let mut start = pos.offset;
        for (i, ch) in text[..pos.offset].char_indices().rev() {
            if class(ch) != k {
                break;
            }
            start = i;
        }
        let start = Position::new(pos.para, start);
        let end = if k == CharClass::Space {
            let mut e = text.len();
            for (i, ch) in text[pos.offset..].char_indices() {
                if class(ch) != CharClass::Space {
                    e = pos.offset + i;
                    break;
                }
            }
            Position::new(pos.para, e)
        } else {
            self.next_word(Position::new(pos.para, start.offset))
        };
        (start, end)
    }

    /// The sentence around `pos` (⌘-click in Word): from after the previous
    /// sentence end to after this one's end and trailing spaces.
    pub fn sentence_at(&self, pos: Position) -> (Position, Position) {
        let text = &self.paras[pos.para].text;
        let is_end = |c: char| matches!(c, '.' | '!' | '?');
        let mut start = 0;
        let mut prev_end = false;
        for (i, c) in text[..pos.offset].char_indices() {
            if prev_end && c.is_whitespace() {
                continue;
            }
            if prev_end {
                start = i;
                prev_end = false;
            }
            if is_end(c) {
                prev_end = true;
            }
        }
        if prev_end {
            start = pos.offset;
        }
        let mut end = text.len();
        let mut seen_end = false;
        for (i, c) in text[pos.offset..].char_indices() {
            if seen_end && !c.is_whitespace() && !is_end(c) {
                end = pos.offset + i;
                break;
            }
            if is_end(c) {
                seen_end = true;
            }
        }
        (Position::new(pos.para, start), Position::new(pos.para, end))
    }

    /// The whole paragraph at `para`, including its mark.
    pub fn paragraph_range(&self, para: usize) -> (Position, Position) {
        let start = Position::new(para, 0);
        let end = self.next_char(Position::new(para, self.paras[para].len()));
        (start, end)
    }

    /// Start of the paragraph after `pos` (⌘↓), or the story end.
    pub fn next_paragraph(&self, pos: Position) -> Position {
        if pos.para + 1 < self.paras.len() {
            Position::new(pos.para + 1, 0)
        } else {
            self.end()
        }
    }

    /// Start of this paragraph, or of the previous one if already there (⌘↑).
    pub fn prev_paragraph(&self, pos: Position) -> Position {
        if pos.offset > 0 || pos.para == 0 {
            Position::new(pos.para, 0)
        } else {
            Position::new(pos.para - 1, 0)
        }
    }

    /// Plain text of a range, paragraphs separated by `\n`.
    pub fn text_of(&self, start: Position, end: Position) -> String {
        let mut out = String::new();
        for p in start.para..=end.para.min(self.paras.len() - 1) {
            let text = &self.paras[p].text;
            let s = if p == start.para { start.offset } else { 0 };
            let e = if p == end.para {
                end.offset
            } else {
                text.len()
            };
            out.push_str(&text[s..e]);
            if p != end.para {
                out.push('\n');
            }
        }
        out
    }

    /// Total characters, counting each paragraph mark as one, the way
    /// Word's Word Count does.
    pub fn char_count(&self) -> usize {
        self.paras.iter().map(|p| p.text.chars().count() + 1).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::{CharProps, ParaProps};

    fn story(paras: &[&str]) -> Story {
        Story {
            paras: paras
                .iter()
                .map(|t| Paragraph::with_text(t, CharProps::default(), ParaProps::default()))
                .collect(),
        }
    }

    fn p(para: usize, offset: usize) -> Position {
        Position::new(para, offset)
    }

    #[test]
    fn char_moves_cross_marks() {
        let s = story(&["ab", "c"]);
        assert_eq!(s.next_char(p(0, 2)), p(1, 0));
        assert_eq!(s.prev_char(p(1, 0)), p(0, 2));
        assert_eq!(s.next_char(p(1, 1)), p(1, 1), "story end stays put");
        assert_eq!(s.prev_char(p(0, 0)), p(0, 0));
    }

    #[test]
    fn char_moves_respect_utf8() {
        let s = story(&["é€x"]);
        assert_eq!(s.next_char(p(0, 0)), p(0, 2));
        assert_eq!(s.next_char(p(0, 2)), p(0, 5));
        assert_eq!(s.prev_char(p(0, 5)), p(0, 2));
    }

    #[test]
    fn word_moves() {
        let s = story(&["Hello Text. The quick"]);
        assert_eq!(s.next_word(p(0, 0)), p(0, 6));
        assert_eq!(s.next_word(p(0, 6)), p(0, 10), "stops before the period");
        assert_eq!(s.next_word(p(0, 10)), p(0, 12));
        assert_eq!(s.prev_word(p(0, 12)), p(0, 10));
        assert_eq!(s.prev_word(p(0, 10)), p(0, 6));
        assert_eq!(s.prev_word(p(0, 3)), p(0, 0));
    }

    #[test]
    fn double_click_selects_word_and_trailing_space() {
        let s = story(&["Hello Text. The"]);
        assert_eq!(s.word_at(p(0, 2)), (p(0, 0), p(0, 6)));
        assert_eq!(s.word_at(p(0, 7)), (p(0, 6), p(0, 10)));
        assert_eq!(s.word_at(p(0, 15)), (p(0, 12), p(0, 15)));
    }

    #[test]
    fn sentence_selection() {
        let s = story(&["One two. Three four! Five"]);
        assert_eq!(s.sentence_at(p(0, 3)), (p(0, 0), p(0, 9)));
        assert_eq!(s.sentence_at(p(0, 12)), (p(0, 9), p(0, 21)));
        assert_eq!(s.sentence_at(p(0, 23)), (p(0, 21), p(0, 25)));
    }

    #[test]
    fn paragraph_moves() {
        let s = story(&["ab", "cd", "ef"]);
        assert_eq!(s.next_paragraph(p(0, 1)), p(1, 0));
        assert_eq!(s.prev_paragraph(p(1, 1)), p(1, 0));
        assert_eq!(s.prev_paragraph(p(1, 0)), p(0, 0));
        assert_eq!(s.next_paragraph(p(2, 0)), p(2, 2));
        assert_eq!(s.paragraph_range(0), (p(0, 0), p(1, 0)));
    }

    #[test]
    fn text_of_range() {
        let s = story(&["Hello", "big", "world"]);
        assert_eq!(s.text_of(p(0, 2), p(2, 3)), "llo\nbig\nwor");
        assert_eq!(s.text_of(p(1, 0), p(1, 3)), "big");
    }

    #[test]
    fn clamp_fixes_bad_positions() {
        let s = story(&["é"]);
        assert_eq!(s.clamp(p(0, 1)), p(0, 0));
        assert_eq!(s.clamp(p(5, 0)), p(0, 2));
    }
}
