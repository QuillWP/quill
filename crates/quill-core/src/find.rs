//! Find and Replace the original way: ^ codes for special characters, ?
//! for any character, and character or paragraph formats to look for or
//! to apply.

use crate::document::{Document, Fragment, StoryId};
use crate::para::{Mark, chars};
use crate::props::{
    Align, CharProps, FontId, LineSpacing, ParaProps, Rgb, StyleId, Twips, Underline,
};
use crate::story::{Position, Story};

macro_rules! spec {
    ($(#[$m:meta])* $name:ident for $props:ty { $($field:ident: $ty:ty),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, Default, PartialEq, Eq)]
        pub struct $name {
            $(pub $field: Option<$ty>,)*
        }

        impl $name {
            pub fn is_empty(&self) -> bool {
                true $(&& self.$field.is_none())*
            }

            pub fn matches(&self, p: &$props) -> bool {
                true $(&& self.$field.as_ref().is_none_or(|v| *v == p.$field))*
            }

            pub fn apply(&self, p: &mut $props) {
                $(if let Some(v) = &self.$field {
                    p.$field = v.clone();
                })*
            }
        }
    };
}

spec!(
    /// Character formats to look for or apply. Each `Some` is a setting
    /// that matters; `None` means any.
    CharSpec for CharProps {
        font: FontId,
        size: u16,
        bold: bool,
        italic: bool,
        underline: Underline,
        strike: bool,
        outline: bool,
        shadow: bool,
        small_caps: bool,
        all_caps: bool,
        hidden: bool,
        color: Rgb,
        position: Twips,
        spacing: Twips,
    }
);

spec!(
    /// Paragraph formats to look for or apply.
    ParaSpec for ParaProps {
        style: StyleId,
        align: Align,
        left: Twips,
        right: Twips,
        first: Twips,
        before: Twips,
        after: Twips,
        line: LineSpacing,
        page_break_before: bool,
        keep_with_next: bool,
        keep_together: bool,
    }
);

impl CharSpec {
    /// Words for the format, as the original shows them under the Find What field:
    /// "Bold, Not Italic, Font: Geneva, 12 Point".
    pub fn describe(&self, font_name: &dyn Fn(FontId) -> String) -> String {
        let mut out: Vec<String> = Vec::new();
        if let Some(f) = self.font {
            out.push(format!("Font: {}", font_name(f)));
        }
        if let Some(s) = self.size {
            let pt = s as f32 / 2.0;
            out.push(format!("{pt} Point"));
        }
        let flags = [
            (self.bold, "Bold"),
            (self.italic, "Italic"),
            (self.strike, "Strikethru"),
            (self.outline, "Outline"),
            (self.shadow, "Shadow"),
            (self.small_caps, "Small Caps"),
            (self.all_caps, "All Caps"),
            (self.hidden, "Hidden"),
        ];
        for (v, name) in flags {
            match v {
                Some(true) => out.push(name.to_string()),
                Some(false) => out.push(format!("Not {name}")),
                None => {}
            }
        }
        if let Some(u) = self.underline {
            out.push(
                match u {
                    Underline::None => "Not Underline",
                    Underline::Single => "Underline",
                    Underline::Word => "Word Underline",
                    Underline::Double => "Double Underline",
                    Underline::Dotted => "Dotted Underline",
                }
                .to_string(),
            );
        }
        if let Some(p) = self.position {
            out.push(
                match p.signum() {
                    1 => "Superscript",
                    -1 => "Subscript",
                    _ => "Not Superscript/Subscript",
                }
                .to_string(),
            );
        }
        if let Some(s) = self.spacing {
            out.push(
                match s.signum() {
                    1 => "Expanded",
                    -1 => "Condensed",
                    _ => "Normal Spacing",
                }
                .to_string(),
            );
        }
        if self.color.is_some() {
            out.push("Color".to_string());
        }
        out.join(", ")
    }
}

impl ParaSpec {
    pub fn describe(&self, style_name: &dyn Fn(StyleId) -> String) -> String {
        let mut out: Vec<String> = Vec::new();
        if let Some(s) = self.style {
            out.push(format!("Style: {}", style_name(s)));
        }
        if let Some(a) = self.align {
            out.push(
                match a {
                    Align::Left => "Flush Left",
                    Align::Center => "Centered",
                    Align::Right => "Flush Right",
                    Align::Justify => "Justified",
                }
                .to_string(),
            );
        }
        if self.left.is_some() || self.right.is_some() || self.first.is_some() {
            out.push("Indent".to_string());
        }
        if self.before.is_some() || self.after.is_some() || self.line.is_some() {
            out.push("Spacing".to_string());
        }
        if self.page_break_before == Some(true) {
            out.push("Page Break Before".to_string());
        }
        if self.keep_with_next == Some(true) {
            out.push("Keep With Next".to_string());
        }
        if self.keep_together == Some(true) {
            out.push("Keep Lines Together".to_string());
        }
        out.join(", ")
    }
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
    pub chp: CharSpec,
    pub pap: ParaSpec,
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
    pub fn new(
        text: &str,
        match_case: bool,
        whole_word: bool,
        chp: CharSpec,
        pap: ParaSpec,
    ) -> Self {
        let mut atoms = Vec::new();
        let mut it = text.chars().peekable();
        while let Some(c) = it.next() {
            let atom = match c {
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
            chp,
            pap,
        }
    }

    /// Nothing to look for: no text and no format.
    pub fn is_empty(&self) -> bool {
        self.atoms.is_empty() && self.chp.is_empty() && self.pap.is_empty()
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

    /// Whether the unit at `pos` has the format looked for.
    fn format_ok(&self, story: &Story, pos: Position, unit: Unit) -> bool {
        let p = &story.paras[pos.para];
        if !self.pap.matches(&p.pap) {
            return false;
        }
        if self.chp.is_empty() {
            return true;
        }
        let chp = match unit {
            Unit::Char(_) => p.chp_of_char(pos.offset),
            Unit::Mark(_) => p.mark_chp,
        };
        self.chp.matches(&chp)
    }

    /// Where a match starting at `start` ends, if there is one.
    fn match_at(&self, story: &Story, start: Position) -> Option<Position> {
        if self.atoms.is_empty() {
            return self.format_run_at(story, start);
        }
        if self.whole_word
            && let Some(before) = prev_pos(story, start)
            && let Some((Unit::Char(c), _)) = unit_at(story, before)
            && is_word_char(c)
            && before.para == start.para
        {
            return None;
        }
        let mut pos = start;
        for &atom in &self.atoms {
            let (unit, next) = unit_at(story, pos)?;
            if !self.atom_matches(atom, story, pos, unit) || !self.format_ok(story, pos, unit) {
                return None;
            }
            pos = next;
            if atom == Atom::White {
                while let Some((u, n)) = unit_at(story, pos) {
                    if matches!(u, Unit::Char(c) if is_white(c)) && self.format_ok(story, pos, u) {
                        pos = n;
                    } else {
                        break;
                    }
                }
            }
        }
        if self.whole_word
            && let Some((Unit::Char(c), _)) = unit_at(story, pos)
            && is_word_char(c)
        {
            return None;
        }
        Some(pos)
    }

    /// Searching for a format alone finds a stretch of text that has it: a
    /// run of characters for a character format, a whole paragraph for a
    /// paragraph format.
    fn format_run_at(&self, story: &Story, start: Position) -> Option<Position> {
        if self.chp.is_empty() && self.pap.is_empty() {
            return None;
        }
        if self.chp.is_empty() {
            if start.offset != 0 || !self.pap.matches(&story.paras[start.para].pap) {
                return None;
            }
            let next = start.para + 1;
            return Some(if next < story.paras.len() {
                Position::new(next, 0)
            } else {
                Position::new(start.para, story.paras[start.para].len())
            });
        }
        let (unit, mut pos) = unit_at(story, start)?;
        if !self.format_ok(story, start, unit) {
            return None;
        }
        while let Some((u, next)) = unit_at(story, pos) {
            let ok = match u {
                Unit::Char(_) => true,
                Unit::Mark(m) => m == Mark::Paragraph,
            };
            if !ok || !self.format_ok(story, pos, u) {
                break;
            }
            pos = next;
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
                    // A format run found from inside it starts where the
                    // run does.
                    let mut start = pos;
                    if pat.atoms.is_empty() {
                        while let Some(p) = prev_pos(story, start) {
                            match pat.match_at(story, p) {
                                Some(e) if e == end => start = p,
                                _ => break,
                            }
                        }
                    }
                    return Some((start, end));
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
    pub chp: CharSpec,
    pub pap: ParaSpec,
}

impl Replacement {
    pub fn new(text: &str, chp: CharSpec, pap: ParaSpec) -> Self {
        let mut pieces = Vec::new();
        let mut buf = String::new();
        let mut it = text.chars();
        while let Some(c) = it.next() {
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
        Replacement { pieces, chp, pap }
    }

    /// Replace With is empty and has no format: matches are deleted.
    pub fn deletes(&self) -> bool {
        self.pieces.is_empty() && self.chp.is_empty() && self.pap.is_empty()
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
    let mut out_end = end;
    if !rep.pieces.is_empty() || rep.deletes() {
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
        out_end = at;
    }
    if !rep.chp.is_empty() {
        doc.format_chars(story, start, out_end, |c| rep.chp.apply(c));
    }
    if !rep.pap.is_empty() {
        doc.format_paras(story, start.para, out_end.para, |p| rep.pap.apply(p));
    }
    (start, out_end)
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
    // `to` is kept as a distance from the end of the story, which edits
    // before it do not change.
    let rel = {
        let s = doc.story(story);
        let to = s.clamp(to);
        (s.paras.len() - to.para, s.paras[to.para].len() - to.offset)
    };
    let limit = |s: &Story| {
        let para = s.paras.len().saturating_sub(rel.0);
        Position::new(para, s.paras[para].len().saturating_sub(rel.1))
    };
    let mut pos = from;
    let mut count = 0;
    loop {
        let s = doc.story(story);
        let to = limit(s);
        let Some((a, b)) = find(s, pat, pos, Direction::Down) else {
            break;
        };
        if b > to {
            break;
        }
        let (_, e) = replace(doc, story, a, b, rep, pat.match_case, clip);
        count += 1;
        pos = e;
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
        Pattern::new(text, false, false, CharSpec::default(), ParaSpec::default())
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
    fn finds_a_format() {
        let mut d = doc(&["plain bold plain"]);
        d.format_chars(StoryId::Main, p(0, 6), p(0, 10), |c| c.bold = true);
        let bold = Pattern::new(
            "",
            false,
            false,
            CharSpec {
                bold: Some(true),
                ..Default::default()
            },
            ParaSpec::default(),
        );
        assert_eq!(
            find(&d.main, &bold, p(0, 0), Direction::Down),
            Some((p(0, 6), p(0, 10)))
        );
        assert_eq!(
            find(&d.main, &bold, p(0, 16), Direction::Up),
            Some((p(0, 6), p(0, 10)))
        );
        let mut plain_l = pat("l");
        plain_l.chp.bold = Some(true);
        assert_eq!(
            find(&d.main, &plain_l, p(0, 0), Direction::Down),
            Some((p(0, 8), p(0, 9)))
        );
    }

    #[test]
    fn replaces_keeping_capitals() {
        let mut d = doc(&["Cat and cat and CAT"]);
        let n = replace_all(
            &mut d,
            StoryId::Main,
            &pat("cat"),
            &Replacement::new("dog", CharSpec::default(), ParaSpec::default()),
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
            &Replacement::new("^p", CharSpec::default(), ParaSpec::default()),
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
            &Replacement::new("^t", CharSpec::default(), ParaSpec::default()),
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
            &Replacement::new("yy", CharSpec::default(), ParaSpec::default()),
            p(0, 2),
            p(0, 5),
            None,
        );
        assert_eq!(n, 2);
        assert_eq!(d.main.paras[0].text, "x yy yy x");
    }

    #[test]
    fn replace_format_only() {
        let mut d = doc(&["one two"]);
        let rep = Replacement::new(
            "",
            CharSpec {
                italic: Some(true),
                ..Default::default()
            },
            ParaSpec::default(),
        );
        let n = replace_all(
            &mut d,
            StoryId::Main,
            &pat("two"),
            &rep,
            p(0, 0),
            p(0, 7),
            None,
        );
        assert_eq!(n, 1);
        assert_eq!(d.main.paras[0].text, "one two");
        assert!(d.main.paras[0].chp_of_char(4).italic);
        assert!(!d.main.paras[0].chp_of_char(0).italic);
    }

    #[test]
    fn deleting_matches() {
        let mut d = doc(&["a-b-c"]);
        let rep = Replacement::new("", CharSpec::default(), ParaSpec::default());
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
        let rep = Replacement::new("<^c>", CharSpec::default(), ParaSpec::default());
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
