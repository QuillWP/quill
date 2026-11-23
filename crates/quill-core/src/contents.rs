//! Insert > Table of Contents and Insert > Index, as the original makes them.
//! The table of contents collects headings (or `.c.` entries, hidden text
//! from Insert > TOC Entry) and goes at the start of the document in a
//! section of its own; the index collects `.i.` entries (Insert > Index
//! Entry) and goes at the end, after a section break. Both are plain text
//! in the original's toc and index styles, made again each time.
//!
//! Entries: `.c.text;` or `.c2.text;` (level 2), `.i.text;`, with
//! `main:sub` for a subentry, `.ib.` or `.ii.` for a bold or italic page
//! number, and `.i(.` ... `.i).` for a page range.

use crate::document::{Document, SectionProps, SectionStart, StoryId};
use crate::para::{Mark, Paragraph, chars};
use crate::props::CharProps;
use crate::story::Position;

/// Table of Contents dialog settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TocOptions {
    /// Collect headings.
    pub outline: bool,
    /// Collect the entries marked with Insert > TOC Entry, in text order
    /// among the headings.
    pub entries: bool,
    pub page_numbers: bool,
    pub from: u8,
    pub to: u8,
}

impl Default for TocOptions {
    fn default() -> Self {
        TocOptions {
            outline: true,
            entries: true,
            page_numbers: true,
            from: 1,
            to: 9,
        }
    }
}

/// Index dialog settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexOptions {
    /// Subentries on their own lines (Nested) or run in after the main
    /// entry.
    pub nested: bool,
    /// Only entries starting with these letters.
    pub letters: Option<(char, char)>,
}

impl Default for IndexOptions {
    fn default() -> Self {
        IndexOptions {
            nested: true,
            letters: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TocEntry {
    pub level: u8,
    pub text: String,
    pub at: Position,
    /// Made with Insert > TOC Entry rather than from a heading.
    pub marked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeMark {
    None,
    Start,
    End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub main: String,
    pub sub: Option<String>,
    pub at: Position,
    pub bold: bool,
    pub italic: bool,
    pub range: RangeMark,
}

/// The text a reader sees: no hidden text, fields or references, tabs
/// and line breaks as spaces.
pub fn visible_text(p: &Paragraph) -> String {
    visible_chars(p).iter().map(|&(c, _)| c).collect()
}

/// `visible_text`, each character with where it is in the paragraph, so a
/// change to the words can be made around marks that are not part of them.
pub fn visible_chars(p: &Paragraph) -> Vec<(char, usize)> {
    let mut out = Vec::new();
    for (s, e, c) in p.run_bounds() {
        if c.hidden {
            continue;
        }
        for (at, ch) in p.text[s..e].char_indices() {
            match ch {
                chars::TAB | chars::LINE_BREAK => out.push((' ', s + at)),
                chars::PAGE_BREAK
                | chars::OBJECT
                | chars::FOOTNOTE
                | chars::FIELD_PAGE
                | chars::FIELD_DATE
                | chars::FIELD_TIME
                | chars::OPTIONAL_HYPHEN => {}
                c => out.push((c, s + at)),
            }
        }
    }
    while out.last().is_some_and(|c| c.0.is_whitespace()) {
        out.pop();
    }
    let blank = out.iter().take_while(|c| c.0.is_whitespace()).count();
    out.drain(..blank);
    out
}

/// The hidden entries of kind `kind` ('c' or 'i') in a paragraph: the
/// characters after the marker letter up to the closing '.', the entry
/// text up to ';' (or the paragraph's end), and where it starts.
fn hidden_entries(p: &Paragraph, kind: char) -> Vec<(String, String, usize)> {
    entry_spans(p, kind)
        .into_iter()
        .map(|e| (e.flags, e.text, e.start))
        .collect()
}

/// A hidden entry in a paragraph: where it starts (the marker), where it
/// ends (after the closing ';', or at the paragraph's end), its flags and
/// its text.
pub struct EntrySpan {
    /// 'i' for an index entry, 'c' for a table of contents entry.
    pub kind: char,
    pub flags: String,
    pub text: String,
    pub start: usize,
    pub end: usize,
}

fn entry_spans(p: &Paragraph, kind: char) -> Vec<EntrySpan> {
    let mut out = Vec::new();
    let text = &p.text;
    let marker = format!(".{kind}");
    let mut from = 0;
    while let Some(k) = text[from..].find(&marker) {
        let at = from + k;
        from = at + marker.len();
        if !p.chp_of_char(at).hidden {
            continue;
        }
        // Flags between the letter and the next '.', such as "2", "b", "(".
        let Some(dot) = text[from..].find('.') else {
            break;
        };
        let flags = text[from..from + dot].to_string();
        if flags.len() > 2 {
            continue;
        }
        let start = from + dot + 1;
        let end = text[start..].find(';').map_or(text.len(), |e| start + e);
        let entry: String = text[start..end]
            .chars()
            .filter(|c| !matches!(*c, chars::FOOTNOTE | chars::OBJECT))
            .collect();
        let stop = if end < text.len() { end + 1 } else { end };
        out.push(EntrySpan {
            kind,
            flags,
            text: entry.trim().to_string(),
            start: at,
            end: stop,
        });
        from = end;
    }
    out
}

/// The index (`.i.text;`) and table of contents (`.c.text;`) entries in a
/// paragraph, for writers that keep them as the other format's own fields.
pub fn marked_entry_spans(p: &Paragraph) -> Vec<EntrySpan> {
    let mut out = entry_spans(p, 'i');
    out.extend(entry_spans(p, 'c'));
    out
}

impl Document {
    /// The paragraphs of a table of contents made earlier: the first
    /// section that holds nothing but contents paragraphs and has text
    /// after it, wherever it is in the document.
    pub fn existing_toc(&self) -> Option<std::ops::Range<usize>> {
        let paras = &self.main.paras;
        let toc = |p: &Paragraph| self.styles.toc_level(p.pap.style).is_some();
        let mut start = 0;
        for (i, p) in paras.iter().enumerate() {
            if p.mark != Mark::SectionBreak {
                continue;
            }
            if i + 1 < paras.len() && paras[start..=i].iter().all(toc) {
                return Some(start..i + 1);
            }
            start = i + 1;
        }
        None
    }

    /// The paragraphs of an index made earlier: the last section that holds
    /// nothing but index paragraphs, ending at its section break. Text
    /// typed after the index is in the section after it, so making the
    /// index again leaves that text alone.
    pub fn existing_index(&self) -> Option<std::ops::Range<usize>> {
        let paras = &self.main.paras;
        let is_index = |p: &Paragraph| self.styles.get(p.pap.style).name.starts_with("index ");
        let mut found = None;
        let mut start = 0;
        for (i, p) in paras.iter().enumerate() {
            if p.mark != Mark::SectionBreak {
                continue;
            }
            let section = &paras[start..=i];
            if section.iter().all(|p| is_index(p) || p.is_empty()) && section.iter().any(is_index) {
                found = Some(start..i + 1);
            }
            start = i + 1;
        }
        found
    }

    /// The paragraphs a table of contents or index may draw from: not
    /// those of the ones already there.
    fn body_paras(&self) -> impl Iterator<Item = usize> + '_ {
        let toc = self.existing_toc().unwrap_or(0..0);
        let index = self.existing_index().unwrap_or(0..0);
        (0..self.main.paras.len()).filter(move |i| !toc.contains(i) && !index.contains(i))
    }

    /// What the table of contents lists.
    pub fn toc_entries(&self, o: &TocOptions) -> Vec<TocEntry> {
        let mut out = Vec::new();
        let in_range = |l: u8| l >= o.from && l <= o.to;
        for i in self.body_paras() {
            let p = &self.main.paras[i];
            if o.outline
                && let Some(level) = self.styles.heading_level(p.pap.style)
            {
                let text = visible_text(p);
                if in_range(level) && !text.is_empty() {
                    out.push(TocEntry {
                        level,
                        text,
                        at: Position::new(i, 0),
                        marked: false,
                    });
                }
            }
            if !o.entries {
                continue;
            }
            for (flags, text, at) in hidden_entries(p, 'c') {
                let level = flags.parse().unwrap_or(1u8).clamp(1, 9);
                if in_range(level) && !text.is_empty() {
                    out.push(TocEntry {
                        level,
                        text,
                        at: Position::new(i, at),
                        marked: true,
                    });
                }
            }
        }
        out
    }

    /// What the index lists, in the order the entries come in the text.
    pub fn index_entries(&self) -> Vec<IndexEntry> {
        let mut out = Vec::new();
        for i in self.body_paras() {
            for (flags, text, at) in hidden_entries(&self.main.paras[i], 'i') {
                if text.is_empty() && flags != ")" {
                    continue;
                }
                let (main, sub) = match text.split_once(':') {
                    Some((m, s)) => (m.trim().to_string(), Some(s.trim().to_string())),
                    None => (text, None),
                };
                out.push(IndexEntry {
                    main,
                    sub: sub.filter(|s| !s.is_empty()),
                    at: Position::new(i, at),
                    bold: flags.contains('b'),
                    italic: flags.contains('i'),
                    range: if flags.contains('(') {
                        RangeMark::Start
                    } else if flags.contains(')') {
                        RangeMark::End
                    } else {
                        RangeMark::None
                    },
                });
            }
        }
        out
    }

    fn styled(&self, name: &str, text: &str) -> Paragraph {
        let id = self.styles.find(name).unwrap_or(0);
        let st = self.styles.get(id);
        let mut p = Paragraph::with_text(text, st.chp, st.pap.clone());
        p.pap.style = id;
        p
    }

    /// The table of contents' paragraphs: each entry, a tab and its page.
    pub fn toc_paragraphs(&self, entries: &[(TocEntry, String)], numbers: bool) -> Vec<Paragraph> {
        entries
            .iter()
            .map(|(e, page)| {
                let text = if numbers {
                    format!("{}\t{}", e.text, page)
                } else {
                    e.text.clone()
                };
                self.styled(&format!("toc {}", e.level), &text)
            })
            .collect()
    }

    /// The index's paragraphs. `pages` gives each entry's page (its
    /// number for sorting and its label).
    pub fn index_paragraphs(
        &self,
        entries: &[(IndexEntry, usize, String)],
        o: &IndexOptions,
    ) -> Vec<Paragraph> {
        // Main entry -> its own pages, and its subentries' pages.
        type Pages = Vec<(usize, String, bool, bool)>;
        type Main = (String, Pages, Vec<(String, Pages)>);
        let mut mains: Vec<Main> = Vec::new();
        let mut open: Vec<(String, Option<String>, usize)> = Vec::new();
        for (e, k, label) in entries {
            let key = |s: &str| s.to_lowercase();
            let first = e.main.chars().next().map(|c| c.to_ascii_uppercase());
            if let (Some((a, b)), Some(f)) = (o.letters, first)
                && !(a.to_ascii_uppercase()..=b.to_ascii_uppercase()).contains(&f)
            {
                continue;
            }
            let label = match e.range {
                RangeMark::Start => {
                    open.push((e.main.clone(), e.sub.clone(), *k));
                    continue;
                }
                RangeMark::End => {
                    let Some(pos) = open
                        .iter()
                        .position(|(m, s, _)| key(m) == key(&e.main) && *s == e.sub)
                        .or(open.len().checked_sub(1))
                    else {
                        continue;
                    };
                    let (_, _, from) = open.remove(pos);
                    if from == *k {
                        label.clone()
                    } else {
                        format!(
                            "{}-{}",
                            entries
                                .iter()
                                .find(|x| x.1 == from)
                                .map_or(String::new(), |x| x.2.clone()),
                            label
                        )
                    }
                }
                RangeMark::None => label.clone(),
            };
            let page = (*k, label, e.bold, e.italic);
            let m = match mains.iter().position(|(m, _, _)| key(m) == key(&e.main)) {
                Some(m) => m,
                None => {
                    mains.push((e.main.clone(), Vec::new(), Vec::new()));
                    mains.len() - 1
                }
            };
            match &e.sub {
                None => mains[m].1.push(page),
                Some(s) => {
                    let subs = &mut mains[m].2;
                    match subs.iter().position(|(x, _)| key(x) == key(s)) {
                        Some(i) => subs[i].1.push(page),
                        None => subs.push((s.clone(), vec![page])),
                    }
                }
            }
        }
        let sort_key = |s: &str| s.to_lowercase();
        mains.sort_by_key(|m| sort_key(&m.0));
        for m in &mut mains {
            m.2.sort_by_key(|s| sort_key(&s.0));
        }
        // "text, 1, 4" with each page in its own format.
        let line = |p: &mut Paragraph, pages: &mut Pages| {
            pages.sort_by_key(|x| x.0);
            pages.dedup_by(|a, b| a.1 == b.1);
            let base = p.chp_at(p.len());
            for (_, label, bold, italic) in pages.iter() {
                let at = p.len();
                p.insert(at, ", ", base);
                let at = p.len();
                p.insert(
                    at,
                    label,
                    CharProps {
                        bold: *bold || base.bold,
                        italic: *italic || base.italic,
                        ..base
                    },
                );
            }
        };
        let mut out = Vec::new();
        // No blank line between letters, as in Word's own index, so updating
        // the INDEX field in Word gives the same paragraphs.
        for (main, mut pages, subs) in mains {
            let mut p = self.styled("index 1", &main);
            line(&mut p, &mut pages);
            if o.nested {
                out.push(p);
                for (sub, mut pages) in subs {
                    let mut s = self.styled("index 2", &sub);
                    line(&mut s, &mut pages);
                    out.push(s);
                }
            } else {
                let mut subs = subs;
                for (k, (sub, pages)) in subs.iter_mut().enumerate() {
                    let at = p.len();
                    let sep = if k == 0 { ": " } else { "; " };
                    let chp = p.chp_at(at);
                    p.insert(at, &format!("{sep}{sub}"), chp);
                    line(&mut p, pages);
                }
                out.push(p);
            }
        }
        if out.is_empty() {
            out.push(self.styled("index 1", "No index entries found."));
        }
        out
    }

    /// Put a table of contents in a section of its own, in place of any
    /// made before. A new one goes at `at`: the text before it ends a
    /// section, and the text after it is in the section that follows.
    /// Returns the paragraphs it takes.
    pub fn place_toc(&mut self, mut paras: Vec<Paragraph>, at: Position) -> std::ops::Range<usize> {
        if paras.is_empty() {
            paras.push(self.styled("toc 1", "No table of contents entries found."));
        }
        let n = paras.len();
        let old = self.existing_toc();
        // The contents section is laid out as one column, with the
        // settings of the section after it.
        let body = match &old {
            Some(r) => self.section_props_at(r.end).clone(),
            None => self.section_props_at(at.para).clone(),
        };
        let last = paras.last_mut().unwrap();
        last.mark = Mark::SectionBreak;
        last.section = Some(Box::new(crate::document::SectionProps {
            columns: 1,
            ..body.clone()
        }));
        if let Some(r) = old {
            let start = r.start;
            self.splice(StoryId::Main, start, r.len(), paras);
            return start..start + n;
        }
        // The text before it is a section that ends where the contents begin.
        let mut before = Vec::new();
        let mut tail = None;
        let para = &self.main.paras[at.para];
        if at.offset > 0 {
            let mut head = para.clone();
            tail = Some(head.split_off(at.offset));
            head.mark = Mark::SectionBreak;
            head.section = Some(Box::new(body.clone()));
            before.push(head);
        } else if at.para > 0 {
            let prev = &self.main.paras[at.para - 1];
            match prev.mark {
                Mark::SectionBreak => {}
                Mark::Paragraph => {
                    let mut p = prev.clone();
                    p.mark = Mark::SectionBreak;
                    p.section = Some(Box::new(body.clone()));
                    self.splice(StoryId::Main, at.para - 1, 1, vec![p]);
                }
                // After a table: a paragraph of its own ends the section.
                _ => {
                    let mut p = Paragraph::new(para.pap.clone(), para.mark_chp);
                    p.mark = Mark::SectionBreak;
                    p.section = Some(Box::new(body.clone()));
                    before.push(p);
                }
            }
        }
        let start = at.para + before.len();
        let mut new = before;
        new.extend(paras);
        match tail {
            Some(tail) => {
                new.push(tail);
                self.splice(StoryId::Main, at.para, 1, new);
            }
            None => self.splice(StoryId::Main, at.para, 0, new),
        }
        start..start + n
    }

    /// Put an index at the end of the document, in a section of its own
    /// with a section after it for whatever is written next, in place of
    /// any made before. Returns where it starts.
    pub fn place_index(&mut self, mut paras: Vec<Paragraph>) -> usize {
        if let Some(old) = self.existing_index() {
            // The new index ends its section as the old one did.
            let props = self.main.paras[old.end - 1].section.clone();
            let mut end = self.styled("index 1", "");
            end.mark = Mark::SectionBreak;
            end.section = props;
            paras.push(end);
            let start = old.start;
            self.splice(StoryId::Main, old.start, old.len(), paras);
            return start;
        }
        let n = self.main.paras.len();
        // The text's last paragraph ends its section, the index is the
        // next one, and the section after it takes over as the last.
        let mut head = self.main.paras[n - 1].clone();
        head.mark = Mark::SectionBreak;
        head.section = Some(Box::new(self.section.clone()));
        // Word's index ends with an empty paragraph that holds the section
        // break and the end of the INDEX field.
        let mut end = self.styled("index 1", "");
        end.mark = Mark::SectionBreak;
        end.section = Some(Box::new(self.section.clone()));
        paras.push(end);
        let normal = self.styles.get(0);
        let tail = Paragraph::new(normal.pap.clone(), normal.chp);
        let mut new = vec![head];
        new.extend(paras);
        new.push(tail);
        self.record_section();
        self.section = SectionProps {
            start: SectionStart::NoBreak,
            headers: None,
            restart_page_numbers: false,
            ..self.section.clone()
        };
        self.splice(StoryId::Main, n - 1, 1, new);
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::StoryId::Main;

    fn doc(lines: &[&str]) -> Document {
        let mut d = crate::text::from_plain_text(&lines.join("\n"));
        let h1 = d.styles.find("heading 1").unwrap();
        let h2 = d.styles.find("heading 2").unwrap();
        for (i, l) in lines.iter().enumerate() {
            if l.starts_with("Chapter") {
                d.apply_style(Main, i, i, h1);
            } else if l.starts_with("Part") {
                d.apply_style(Main, i, i, h2);
            }
        }
        d
    }

    /// Hide `marker` characters inside paragraph `i`.
    fn hide(d: &mut Document, i: usize, marker: &str) {
        let mut from = 0;
        while let Some(k) = d.main.paras[i].text[from..].find(marker) {
            let s = from + k;
            d.format_chars(
                Main,
                Position::new(i, s),
                Position::new(i, s + marker.len()),
                |c| c.hidden = true,
            );
            from = s + marker.len();
        }
    }

    #[test]
    fn toc_lists_headings_and_marked_entries_in_text_order() {
        let mut d = doc(&["Chapter One", "A side note", "Chapter Two", "end"]);
        // Mark the second paragraph as a TOC entry.
        d.insert_text(
            Main,
            Position::new(1, 0),
            ".c.Side note;",
            CharProps::default(),
        );
        hide(&mut d, 1, ".c.");
        hide(&mut d, 1, ";");
        let both = d.toc_entries(&TocOptions::default());
        let texts: Vec<_> = both.iter().map(|e| (e.text.as_str(), e.marked)).collect();
        assert_eq!(
            texts,
            [
                ("Chapter One", false),
                ("Side note", true),
                ("Chapter Two", false)
            ]
        );
        let headings = d.toc_entries(&TocOptions {
            entries: false,
            ..Default::default()
        });
        assert_eq!(headings.len(), 2);
        let marked = d.toc_entries(&TocOptions {
            outline: false,
            ..Default::default()
        });
        assert_eq!(marked.len(), 1);
    }

    #[test]
    fn toc_from_headings_goes_where_asked_and_replaces_itself() {
        let mut d = doc(&["Chapter One", "text", "Part A", "Chapter Two", "end"]);
        let entries = d.toc_entries(&TocOptions::default());
        let texts: Vec<_> = entries.iter().map(|e| (e.level, e.text.as_str())).collect();
        assert_eq!(
            texts,
            [(1, "Chapter One"), (2, "Part A"), (1, "Chapter Two")]
        );
        let with: Vec<_> = entries.into_iter().map(|e| (e, "1".to_string())).collect();
        let paras = d.toc_paragraphs(&with, true);
        d.place_toc(paras, Position::new(0, 0));
        assert_eq!(d.main.paras[0].text, "Chapter One\t1");
        assert_eq!(d.existing_toc(), Some(0..3));
        assert_eq!(d.main.paras[3].text, "Chapter One");
        // Made again, it replaces the first one.
        let again = d.toc_entries(&TocOptions {
            to: 1,
            ..Default::default()
        });
        assert_eq!(again.len(), 2);
        let with: Vec<_> = again.into_iter().map(|e| (e, "2".to_string())).collect();
        let paras = d.toc_paragraphs(&with, true);
        d.place_toc(paras, Position::new(0, 0));
        assert_eq!(d.existing_toc(), Some(0..2));
        assert_eq!(d.main.paras[2].text, "Chapter One");
    }

    #[test]
    fn index_sorts_merges_and_nests() {
        let mut d = doc(&[
            ".i.Zebra;zebras",
            ".i.cat:tabby;x .ib.Cat;y",
            ".i.Apple; .i.cat:calico;",
        ]);
        for i in 0..3 {
            for m in [".i.", ".ib.", ";"] {
                hide(&mut d, i, m);
            }
        }
        let e = d.index_entries();
        assert_eq!(e.len(), 5);
        let pages: Vec<_> = e
            .into_iter()
            .map(|x| {
                let k = x.at.para + 1;
                (x, k, k.to_string())
            })
            .collect();
        let nested = d.index_paragraphs(&pages, &IndexOptions::default());
        let t: Vec<_> = nested.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            t,
            ["Apple, 3", "cat, 2", "calico, 3", "tabby, 2", "Zebra, 1"]
        );
        assert!(nested[1].chp_of_char(5).bold);
        let run_in = d.index_paragraphs(
            &pages,
            &IndexOptions {
                nested: false,
                letters: Some(('a', 'c')),
            },
        );
        let t: Vec<_> = run_in.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(t, ["Apple, 3", "cat, 2: calico, 3; tabby, 2"]);
        let start = d.place_index(nested);
        assert_eq!(d.existing_index().map(|r| r.start), Some(start));
        assert_eq!(d.main.paras[start - 1].mark, Mark::SectionBreak);
    }

    #[test]
    fn text_written_after_the_index_survives_making_it_again() {
        let mut d = doc(&["Body", ".i.Zebra;"]);
        hide(&mut d, 1, ".i.");
        hide(&mut d, 1, ";");
        let remake = |d: &mut Document| {
            let pages: Vec<_> = d
                .index_entries()
                .into_iter()
                .map(|e| (e, 1, "1".to_string()))
                .collect();
            let paras = d.index_paragraphs(&pages, &IndexOptions::default());
            d.place_index(paras)
        };
        remake(&mut d);
        // The index ends its own section, and a section follows it.
        let range = d.existing_index().unwrap();
        assert_eq!(d.main.paras[range.end - 1].mark, Mark::SectionBreak);
        assert_eq!(range.end, d.main.paras.len() - 1);
        // Writing at the end of the document is after the index.
        let last = d.main.paras.len() - 1;
        d.insert_text(
            Main,
            Position::new(last, 0),
            "After the index",
            CharProps::default(),
        );
        // Text typed at the end of the last index line is part of the
        // index, as in Word, but the section after it is not.
        remake(&mut d);
        let texts: Vec<_> = d.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert!(texts.contains(&"After the index"), "{texts:?}");
        assert_eq!(texts.iter().filter(|t| t.starts_with("Zebra")).count(), 1);
        // An entry typed after the index still counts.
        d.insert_text(
            Main,
            Position::new(d.main.paras.len() - 1, 0),
            ".i.Yak;",
            CharProps::default(),
        );
        let n = d.main.paras.len() - 1;
        hide(&mut d, n, ".i.");
        hide(&mut d, n, ";");
        assert_eq!(d.index_entries().len(), 2);
    }
}
