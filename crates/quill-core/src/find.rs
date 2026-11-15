//! Find and Replace the original way: ^ codes for special characters and ?
//! for any character.

use crate::document::{Document, Fragment, StoryId};
use crate::para::{Mark, chars};
use crate::props::{CharProps, FontId, Rgb, Script, Twips, Underline};
use crate::story::{Position, Story};

/// Character formats a character style sets. Each `Some` is a setting the
/// style has; `None` means it leaves that to the paragraph.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CharSpec {
    pub font: Option<FontId>,
    pub size: Option<u16>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<Underline>,
    pub strike: Option<bool>,
    pub outline: Option<bool>,
    pub shadow: Option<bool>,
    pub small_caps: Option<bool>,
    pub all_caps: Option<bool>,
    pub hidden: Option<bool>,
    pub color: Option<Rgb>,
    pub position: Option<Twips>,
    pub spacing: Option<Twips>,
    pub script: Option<Script>,
}

/// One step of a search pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Atom {
    Char(char),
    /// `?`: any one character.
    Any,
    /// `^*`: any letter.
    Letter,
    /// `^#`: any digit.
    Digit,
    /// `^w`: one or more spaces, tabs and the like.
    White,
    /// `^p`: a paragraph mark.
    Para,
    /// `^d`: a page or section break.
    Break,
    /// `^1`: a picture.
    Graphic,
    /// `^5`: a footnote reference.
    Footnote,
    /// `^\`: a formula character.
    Formula,
}

/// What to look for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pattern {
    atoms: Vec<Atom>,
    pub match_case: bool,
    pub whole_word: bool,
    /// Look past text deleted with Track Changes, as when the window
    /// hides it: a match neither starts in it nor counts it, and a word
    /// split by a hidden deletion is found whole.
    pub skip_deleted: bool,
}

/// Which way to look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
}

/// The special codes Find understands, in the Special menu's order, with
/// their names.
pub const FIND_SPECIALS: [(&str, &str); 15] = [
    ("Tab Mark", "^t"),
    ("End of Line Mark", "^n"),
    ("Paragraph Mark", "^p"),
    ("Page Break", "^d"),
    ("Non-breaking Space", "^s"),
    ("Optional Hyphen", "^-"),
    ("Question Mark", "^?"),
    ("Footnote Mark", "^5"),
    ("Graphic", "^1"),
    ("White Space", "^w"),
    ("Unspecified Letter", "^*"),
    ("Unspecified Digit", "^#"),
    ("Caret (^)", "^^"),
    ("Formula Character", "^\\"),
    ("Any Character", "?"),
];

/// The codes Replace With understands.
pub const REPLACE_SPECIALS: [(&str, &str); 8] = [
    ("Tab Mark", "^t"),
    ("End of Line Mark", "^n"),
    ("Paragraph Mark", "^p"),
    ("Page Break", "^d"),
    ("Non-breaking Space", "^s"),
    ("Optional Hyphen", "^-"),
    ("Question Mark", "^?"),
    ("Clipboard Contents", "^c"),
];

/// The character a plain ^ code stands for, shared by Find and Replace.
fn code_char(c: char) -> Option<char> {
    Some(match c {
        't' => chars::TAB,
        'n' => chars::LINE_BREAK,
        's' => chars::NONBREAKING_SPACE,
        '-' => chars::OPTIONAL_HYPHEN,
        '~' => chars::NONBREAKING_HYPHEN,
        '?' => '?',
        '^' => '^',
        _ => return None,
    })
}

/// Whether text typed in Find What uses wildcards or special characters,
/// which turns off Match Whole Word Only.
pub fn has_specials(text: &str) -> bool {
    text.contains(['^', '?'])
}

impl Pattern {
    pub fn new(text: &str, match_case: bool, whole_word: bool) -> Self {
        let mut atoms = Vec::new();
        let mut it = text.chars().peekable();
        while let Some(c) = it.next() {
            let atom = match c {
                // A line end typed or pasted into Find What is a paragraph
                // mark, as ^p is; a CR LF pair is one.
                '\n' | '\u{2029}' => Atom::Para,
                '\r' => {
                    it.next_if_eq(&'\n');
                    Atom::Para
                }
                '?' => Atom::Any,
                '^' => match it.next() {
                    None => Atom::Char('^'),
                    Some(k) => match k {
                        'p' => Atom::Para,
                        'd' => Atom::Break,
                        'w' => Atom::White,
                        '*' => Atom::Letter,
                        '#' => Atom::Digit,
                        '1' => Atom::Graphic,
                        '5' => Atom::Footnote,
                        '\\' => Atom::Formula,
                        k => match code_char(k) {
                            Some(ch) => Atom::Char(ch),
                            // An unknown code stands for itself.
                            None => {
                                atoms.push(Atom::Char('^'));
                                Atom::Char(k)
                            }
                        },
                    },
                },
                c => Atom::Char(c),
            };
            atoms.push(atom);
        }
        Pattern {
            atoms,
            match_case,
            whole_word,
            skip_deleted: false,
        }
    }

    /// Nothing to look for: no text.
    pub fn is_empty(&self) -> bool {
        self.atoms.is_empty()
    }
}

/// What sits at a position: a character, or the paragraph's mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Char(char),
    Mark(Mark),
}

/// The unit at `pos` and the position after it. The story's last mark is
/// left out: it can be neither found nor replaced.
fn unit_at(story: &Story, pos: Position) -> Option<(Unit, Position)> {
    let p = story.paras.get(pos.para)?;
    if pos.offset < p.len() {
        let c = p.text[pos.offset..].chars().next()?;
        return Some((
            Unit::Char(c),
            Position::new(pos.para, pos.offset + c.len_utf8()),
        ));
    }
    if pos.para + 1 < story.paras.len() {
        return Some((Unit::Mark(p.mark), Position::new(pos.para + 1, 0)));
    }
    None
}

/// The position before `pos`, stepping back over a mark.
fn prev_pos(story: &Story, pos: Position) -> Option<Position> {
    if pos.offset > 0 {
        let p = &story.paras[pos.para];
        let c = p.text[..pos.offset].chars().next_back()?;
        return Some(Position::new(pos.para, pos.offset - c.len_utf8()));
    }
    if pos.para == 0 {
        return None;
    }
    Some(Position::new(pos.para - 1, story.paras[pos.para - 1].len()))
}

/// Whether the unit at `pos` is text, or a paragraph mark, deleted with
/// Track Changes.
fn deleted_at(story: &Story, pos: Position) -> bool {
    let Some(p) = story.paras.get(pos.para) else {
        return false;
    };
    let rev = if pos.offset < p.len() {
        p.chp_of_char(pos.offset).rev
    } else if pos.para + 1 < story.paras.len() {
        p.mark_chp.rev
    } else {
        return false;
    };
    crate::revision::is_deleted(rev)
}

fn is_white(c: char) -> bool {
    matches!(
        c,
        ' ' | chars::TAB | chars::NONBREAKING_SPACE | chars::LINE_BREAK | chars::PAGE_BREAK
    )
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '’' || c == '_'
}

impl Pattern {
    /// `pos`, moved on past any deleted text when that is skipped.
    fn skip(&self, story: &Story, mut pos: Position) -> Position {
        while self.skip_deleted && deleted_at(story, pos) {
            match unit_at(story, pos) {
                Some((_, next)) => pos = next,
                None => break,
            }
        }
        pos
    }

    /// The position before `pos`, stepping back over deleted text when
    /// that is skipped.
    fn prev(&self, story: &Story, pos: Position) -> Option<Position> {
        let mut at = prev_pos(story, pos)?;
        while self.skip_deleted && deleted_at(story, at) {
            at = prev_pos(story, at)?;
        }
        Some(at)
    }

    fn atom_matches(&self, atom: Atom, story: &Story, pos: Position, unit: Unit) -> bool {
        let p = &story.paras[pos.para];
        match (atom, unit) {
            (Atom::Char(a), Unit::Char(c)) => a == c || (!self.match_case && fold(a) == fold(c)),
            (Atom::Any, Unit::Char(c)) => c != chars::OBJECT,
            (Atom::Letter, Unit::Char(c)) => c.is_alphabetic(),
            (Atom::Digit, Unit::Char(c)) => c.is_ascii_digit(),
            (Atom::White, Unit::Char(c)) => is_white(c),
            (Atom::Para, Unit::Mark(Mark::Paragraph)) => true,
            (Atom::Break, Unit::Char(c)) => c == chars::PAGE_BREAK,
            (Atom::Break, Unit::Mark(Mark::SectionBreak)) => true,
            (Atom::Footnote, Unit::Char(c)) => c == chars::FOOTNOTE,
            (Atom::Graphic | Atom::Formula, Unit::Char(chars::OBJECT)) => {
                p.chp_of_char(pos.offset).object != 0
            }
            _ => false,
        }
    }

    /// Where a match starting at `start` ends, if there is one.
    fn match_at(&self, story: &Story, start: Position) -> Option<Position> {
        if self.skip_deleted && deleted_at(story, start) {
            return None;
        }
        if self.atoms.is_empty() {
            return None;
        }
        if self.whole_word
            && let Some(before) = self.prev(story, start)
            && let Some((Unit::Char(c), _)) = unit_at(story, before)
            && is_word_char(c)
            && before.para == start.para
        {
            return None;
        }
        let mut pos = start;
        for &atom in &self.atoms {
            pos = self.skip(story, pos);
            let (unit, next) = unit_at(story, pos)?;
            if !self.atom_matches(atom, story, pos, unit) {
                return None;
            }
            pos = next;
            if atom == Atom::White {
                loop {
                    let at = self.skip(story, pos);
                    let Some((u, n)) = unit_at(story, at) else {
                        break;
                    };
                    if matches!(u, Unit::Char(c) if is_white(c)) {
                        pos = n;
                    } else {
                        break;
                    }
                }
            }
        }
        if self.whole_word
            && let Some((Unit::Char(c), _)) = unit_at(story, self.skip(story, pos))
            && is_word_char(c)
        {
            return None;
        }
        Some(pos)
    }
}

/// The next match from `from` going down (starting at or after it) or up
/// (starting before it).
pub fn find(
    story: &Story,
    pat: &Pattern,
    from: Position,
    dir: Direction,
) -> Option<(Position, Position)> {
    if pat.is_empty() {
        return None;
    }
    let from = story.clamp(from);
    match dir {
        Direction::Down => {
            let mut pos = from;
            loop {
                if let Some(end) = pat.match_at(story, pos)
                    && end > pos
                {
                    return Some((pos, end));
                }
                pos = unit_at(story, pos)?.1;
            }
        }
        Direction::Up => {
            let mut pos = from;
            loop {
                pos = prev_pos(story, pos)?;
                if let Some(end) = pat.match_at(story, pos)
                    && end > pos
                {
                    return Some((pos, end));
                }
            }
        }
    }
}

/// One piece of the replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Text(String),
    Para,
    Clipboard,
}

/// What to put in place of each match.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replacement {
    pieces: Vec<Piece>,
}

impl Replacement {
    pub fn new(text: &str) -> Self {
        let mut pieces = Vec::new();
        let mut buf = String::new();
        let mut it = text.chars().peekable();
        while let Some(c) = it.next() {
            // A line end typed or pasted into Replace With starts a new
            // paragraph, as ^p does; a CR LF pair is one.
            if matches!(c, '\n' | '\r' | '\u{2029}') {
                if c == '\r' {
                    it.next_if_eq(&'\n');
                }
                if !buf.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut buf)));
                }
                pieces.push(Piece::Para);
                continue;
            }
            if c != '^' {
                buf.push(c);
                continue;
            }
            match it.next() {
                None => buf.push('^'),
                Some('d') => buf.push(chars::PAGE_BREAK),
                Some(k @ ('p' | 'c')) => {
                    if !buf.is_empty() {
                        pieces.push(Piece::Text(std::mem::take(&mut buf)));
                    }
                    pieces.push(if k == 'p' {
                        Piece::Para
                    } else {
                        Piece::Clipboard
                    });
                }
                Some(k) => match code_char(k) {
                    Some(ch) => buf.push(ch),
                    None => {
                        buf.push('^');
                        buf.push(k);
                    }
                },
            }
        }
        if !buf.is_empty() {
            pieces.push(Piece::Text(buf));
        }
        Replacement { pieces }
    }

    /// Replace With is empty: matches are deleted.
    pub fn deletes(&self) -> bool {
        self.pieces.is_empty()
    }
}

/// Give `text` the capitals of `found`, as the original does when Match Case is
/// off: all capitals stay all capitals, a capital first letter stays one.
fn match_capitals(found: &str, text: &str) -> String {
    let letters: Vec<char> = found.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return text.to_string();
    }
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return text.to_uppercase();
    }
    if letters[0].is_uppercase() {
        let mut out = String::new();
        let mut done = false;
        for c in text.chars() {
            if !done && c.is_alphabetic() {
                out.extend(c.to_uppercase());
                done = true;
            } else {
                out.push(c);
            }
        }
        return out;
    }
    text.to_string()
}

/// Replace `start..end` (a match) in a story. Returns the range the
/// replacement now covers.
pub fn replace(
    doc: &mut Document,
    story: StoryId,
    start: Position,
    end: Position,
    rep: &Replacement,
    match_case: bool,
    clip: Option<&Fragment>,
) -> (Position, Position) {
    let (start, end) = (start.min(end), start.max(end));
    let found = doc.story(story).text_of(start, end);
    let p = &doc.story(story).paras[start.para];
    let chp: CharProps = if start.offset < p.len() {
        p.chp_of_char(start.offset)
    } else {
        p.chp_at(start.offset)
    };
    let mut at = doc.delete(story, start, end);
    let mut first_text = true;
    for piece in &rep.pieces {
        at = match piece {
            Piece::Text(t) => {
                let t = if !match_case && first_text {
                    match_capitals(&found, t)
                } else {
                    t.clone()
                };
                first_text = false;
                doc.insert_text(story, at, &t, chp)
            }
            Piece::Para => doc.split_paragraph(story, at),
            Piece::Clipboard => match clip {
                Some(f) => doc.insert_fragment(story, at, f),
                None => at,
            },
        };
    }
    let out_end = at;
    (start, out_end)
}

/// A position kept as its distance from the end of the story. Edits
/// before it do not move it, so it can be found again after a replacement
/// earlier in the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FromEnd(usize, usize);

impl FromEnd {
    pub fn new(s: &Story, at: Position) -> FromEnd {
        let at = s.clamp(at);
        FromEnd(s.paras.len() - at.para, s.paras[at.para].len() - at.offset)
    }

    pub fn get(self, s: &Story) -> Position {
        let para = s.paras.len().saturating_sub(self.0);
        Position::new(para, s.paras[para].len().saturating_sub(self.1))
    }
}

/// Replace every match in `from..to`. Returns how many were replaced.
pub fn replace_all(
    doc: &mut Document,
    story: StoryId,
    pat: &Pattern,
    rep: &Replacement,
    from: Position,
    to: Position,
    clip: Option<&Fragment>,
) -> usize {
    let limit = FromEnd::new(doc.story(story), to);
    let mut pos = from;
    let mut count = 0;
    loop {
        let s = doc.story(story);
        let Some((a, b)) = find(s, pat, pos, Direction::Down) else {
            break;
        };
        if b > limit.get(s) {
            break;
        }
        // While tracking, the replaced text stays behind, struck through,
        // after the replacement. Go on from past it, or it is found again
        // and replaced forever.
        let past = FromEnd::new(s, b);
        let (_, e) = replace(doc, story, a, b, rep, pat.match_case, clip);
        count += 1;
        pos = e.max(past.get(doc.story(story)));
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::para::Paragraph;
    use crate::props::ParaProps;

    fn doc(paras: &[&str]) -> Document {
        let mut d = Document::new();
        let chp = d.main.paras[0].mark_chp;
        d.main.paras = paras
            .iter()
            .map(|t| Paragraph::with_text(t, chp, ParaProps::default()))
            .collect();
        d
    }

    fn pat(text: &str) -> Pattern {
        Pattern::new(text, false, false)
    }

    fn p(para: usize, offset: usize) -> Position {
        Position::new(para, offset)
    }

    #[test]
    fn finds_text_ignoring_case() {
        let d = doc(&["The cat sat on the mat"]);
        let f = find(&d.main, &pat("the"), p(0, 1), Direction::Down);
        assert_eq!(f, Some((p(0, 15), p(0, 18))));
        let f = find(&d.main, &pat("the"), p(0, 15), Direction::Up);
        assert_eq!(f, Some((p(0, 0), p(0, 3))));
        let mut case = pat("the");
        case.match_case = true;
        assert_eq!(
            find(&d.main, &case, p(0, 0), Direction::Down),
            Some((p(0, 15), p(0, 18)))
        );
    }

    #[test]
    fn line_ends_in_find_what_and_replace_with_are_paragraph_marks() {
        let spec = |text: &str| Replacement::new(text);
        let x = Pattern::new("x", false, false);
        for text in ["a\nb", "a\r\nb", "a\rb", "a\u{2029}b", "a^pb"] {
            let mut d = doc(&["one x two x three"]);
            let len = d.main.paras[0].len();
            let n = replace_all(
                &mut d,
                StoryId::Main,
                &x,
                &spec(text),
                p(0, 0),
                p(0, len),
                None,
            );
            assert_eq!(n, 2);
            let paras: Vec<&str> = d.main.paras.iter().map(|q| q.text.as_str()).collect();
            assert_eq!(paras, ["one a", "b two a", "b three"], "{text:?}");
        }
        // Two empty lines are two paragraph marks, a blank paragraph between.
        for text in ["\n\n", "\r\n\r\n", "^p^p"] {
            let mut d = doc(&["a x b"]);
            let len = d.main.paras[0].len();
            replace_all(
                &mut d,
                StoryId::Main,
                &x,
                &spec(text),
                p(0, 0),
                p(0, len),
                None,
            );
            let paras: Vec<&str> = d.main.paras.iter().map(|q| q.text.as_str()).collect();
            assert_eq!(paras, ["a ", "", " b"], "{text:?}");
        }
        // Find What reads them the same way.
        let d = doc(&["first", "second"]);
        for text in ["t\ns", "t\r\ns", "t\rs", "t\u{2029}s", "t^ps"] {
            let pat = Pattern::new(text, false, false);
            assert_eq!(
                find(&d.main, &pat, p(0, 0), Direction::Down),
                Some((p(0, 4), p(1, 1))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn hidden_deletions_are_looked_past() {
        let mut d = doc(&["an OLDword here"]);
        d.props.track_changes = true;
        d.author = "Ada".into();
        d.delete(StoryId::Main, p(0, 3), p(0, 6));
        let find_in = |d: &Document, text: &str, skip: bool, whole: bool| {
            let mut pat = Pattern::new(text, false, whole);
            pat.skip_deleted = skip;
            find(&d.main, &pat, p(0, 0), Direction::Down)
        };
        // Shown, the deleted text is found like any other.
        assert_eq!(find_in(&d, "old", false, false), Some((p(0, 3), p(0, 6))));
        assert_eq!(find_in(&d, "an word", false, false), None);
        // Hidden, Find reads what the reader sees.
        assert_eq!(find_in(&d, "old", true, false), None);
        assert_eq!(
            find_in(&d, "an word", true, false),
            Some((p(0, 0), p(0, 10)))
        );
        assert_eq!(find_in(&d, "word", true, true), Some((p(0, 6), p(0, 10))));
        assert_eq!(find_in(&d, "word", false, true), None);
        // Replacing across the hidden text, while tracking, leaves only
        // the replacement once the changes are accepted.
        let mut pat = Pattern::new("an word", false, false);
        pat.skip_deleted = true;
        let rep = Replacement::new("one");
        let end = d.main.end();
        assert_eq!(
            replace_all(&mut d, StoryId::Main, &pat, &rep, p(0, 0), end, None),
            1
        );
        let accepted = crate::tracking::accepted(&d.main.paras);
        assert_eq!(accepted[0].text, "one here");
    }

    #[test]
    fn whole_words_only() {
        let d = doc(&["category cat scat"]);
        let mut w = pat("cat");
        w.whole_word = true;
        assert_eq!(
            find(&d.main, &w, p(0, 0), Direction::Down),
            Some((p(0, 9), p(0, 12)))
        );
        assert_eq!(find(&d.main, &w, p(0, 10), Direction::Down), None);
    }

    #[test]
    fn wildcards_and_codes() {
        let d = doc(&["a1 b22", "next"]);
        assert_eq!(
            find(&d.main, &pat("^*^#"), p(0, 0), Direction::Down),
            Some((p(0, 0), p(0, 2)))
        );
        assert_eq!(
            find(&d.main, &pat("b?2"), p(0, 0), Direction::Down),
            Some((p(0, 3), p(0, 6)))
        );
        // ^p crosses into the next paragraph.
        assert_eq!(
            find(&d.main, &pat("2^pn"), p(0, 0), Direction::Down),
            Some((p(0, 5), p(1, 1)))
        );
        assert_eq!(
            find(&d.main, &pat("1^wb"), p(0, 0), Direction::Down),
            Some((p(0, 1), p(0, 4)))
        );
        let q = doc(&["why? yes"]);
        assert_eq!(
            find(&q.main, &pat("^?"), p(0, 0), Direction::Down),
            Some((p(0, 3), p(0, 4)))
        );
    }

    #[test]
    fn replaces_keeping_capitals() {
        let mut d = doc(&["Cat and cat and CAT"]);
        let n = replace_all(
            &mut d,
            StoryId::Main,
            &pat("cat"),
            &Replacement::new("dog"),
            p(0, 0),
            p(0, 19),
            None,
        );
        assert_eq!(n, 3);
        assert_eq!(d.main.paras[0].text, "Dog and dog and DOG");
    }

    #[test]
    fn replaces_with_paragraph_marks_and_tabs() {
        let mut d = doc(&["a,b,c"]);
        let end = Position::new(0, 5);
        let n = replace_all(
            &mut d,
            StoryId::Main,
            &pat(","),
            &Replacement::new("^p"),
            p(0, 0),
            end,
            None,
        );
        assert_eq!(n, 2);
        let texts: Vec<&str> = d.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["a", "b", "c"]);
        let n = replace_all(
            &mut d,
            StoryId::Main,
            &pat("^p"),
            &Replacement::new("^t"),
            p(0, 0),
            p(2, 1),
            None,
        );
        assert_eq!(n, 2);
        assert_eq!(d.main.paras[0].text, "a\tb\tc");
    }

    #[test]
    fn replace_all_stays_inside_the_range() {
        let mut d = doc(&["x x x x"]);
        let n = replace_all(
            &mut d,
            StoryId::Main,
            &pat("x"),
            &Replacement::new("yy"),
            p(0, 2),
            p(0, 5),
            None,
        );
        assert_eq!(n, 2);
        assert_eq!(d.main.paras[0].text, "x yy yy x");
    }

    #[test]
    fn deleting_matches() {
        let mut d = doc(&["a-b-c"]);
        let rep = Replacement::new("");
        assert!(rep.deletes());
        let n = replace_all(
            &mut d,
            StoryId::Main,
            &pat("-"),
            &rep,
            p(0, 0),
            p(0, 5),
            None,
        );
        assert_eq!(n, 2);
        assert_eq!(d.main.paras[0].text, "abc");
    }

    #[test]
    fn clipboard_contents() {
        let mut d = doc(&["[x]"]);
        let chp = d.main.paras[0].mark_chp;
        let frag = Fragment {
            paras: vec![Paragraph::with_text("clip", chp, ParaProps::default())],
            ends_with_mark: false,
            objects: Vec::new(),
            notes: Vec::new(),
        };
        let rep = Replacement::new("<^c>");
        replace_all(
            &mut d,
            StoryId::Main,
            &pat("x"),
            &rep,
            p(0, 0),
            p(0, 3),
            Some(&frag),
        );
        assert_eq!(d.main.paras[0].text, "[<clip>]");
    }
}
