//! The thesaurus: for a word, its meanings (each with a part of speech
//! and a list of synonyms), as the original's Thesaurus dialog shows them.
//! Quill's data has one line per word:
//! `word<TAB>pos|synonym|synonym<TAB>pos|...`, sorted ignoring case.

pub struct Thesaurus {
    text: String,
    /// Where each line starts, in order of its word, ignoring case.
    starts: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meaning {
    /// "noun", "verb", "adj", "adv", or "antonym" for the opposites.
    pub pos: String,
    pub words: Vec<String>,
}

impl Meaning {
    /// How the Meanings list names it: its first synonym and its part of
    /// speech, as "joyful (adj.)"; antonyms as "Antonyms".
    pub fn label(&self) -> String {
        if self.pos == "antonym" {
            return "Antonyms".into();
        }
        let first = self.words.first().map_or("", String::as_str);
        format!("{first} ({})", self.pos)
    }
}

impl Thesaurus {
    pub fn from_text(text: String) -> Self {
        let mut starts = Vec::new();
        let mut at = 0;
        for line in text.split_inclusive('\n') {
            if !line.trim().is_empty() {
                starts.push(at as u32);
            }
            at += line.len();
        }
        Thesaurus { text, starts }
    }

    fn line(&self, k: usize) -> &str {
        let s = self.starts[k] as usize;
        let e = self.text[s..].find('\n').map_or(self.text.len(), |e| s + e);
        &self.text[s..e]
    }

    fn head(&self, k: usize) -> &str {
        let l = self.line(k);
        l.split('\t').next().unwrap_or(l)
    }

    fn find(&self, word: &str) -> Option<usize> {
        let key = word.to_lowercase();
        let (mut lo, mut hi) = (0, self.starts.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.head(mid).to_lowercase().cmp(&key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    /// Words listed near `word` (where it would come), for an unknown
    /// word's Alphabetical List.
    pub fn nearby(&self, word: &str, n: usize) -> Vec<String> {
        let key = word.to_lowercase();
        let (mut lo, mut hi) = (0, self.starts.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.head(mid).to_lowercase() < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let at = lo;
        let from = at.saturating_sub(n / 4);
        (from..(from + n).min(self.starts.len()))
            .map(|k| self.head(k).to_string())
            .collect()
    }

    /// The meanings of `word`, or of its plain form when it has an
    /// ending ("cats", "running", "happier"), with the word found.
    pub fn look_up(&self, word: &str) -> Option<(String, Vec<Meaning>)> {
        let w = word.trim().to_lowercase();
        let mut tries = vec![w.clone()];
        for (end, add) in [
            ("ies", "y"),
            ("es", ""),
            ("s", ""),
            ("ied", "y"),
            ("ed", ""),
            ("ed", "e"),
            ("ing", ""),
            ("ing", "e"),
            ("ier", "y"),
            ("iest", "y"),
            ("er", ""),
            ("est", ""),
            ("ly", ""),
        ] {
            if let Some(stem) = w.strip_suffix(end)
                && stem.len() > 2
            {
                tries.push(format!("{stem}{add}"));
            }
        }
        let k = tries.iter().find_map(|t| self.find(t))?;
        let line = self.line(k);
        let mut parts = line.split('\t');
        let head = parts.next().unwrap_or("").to_string();
        let meanings = parts
            .map(|g| {
                let mut f = g.split('|');
                Meaning {
                    pos: f.next().unwrap_or("").to_string(),
                    words: f.map(str::to_string).collect(),
                }
            })
            .collect();
        Some((head, meanings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_words_and_their_plain_forms() {
        let t = Thesaurus::from_text(
            "big\tadj|large|huge\tantonym|small\nCat\tnoun|true cat\nhappy\tadj|glad\n".into(),
        );
        let (head, m) = t.look_up("Big").unwrap();
        assert_eq!(head, "big");
        assert_eq!(m[0].label(), "large (adj)");
        assert_eq!(t.nearby("cab", 4), ["big", "Cat", "happy"]);
        assert_eq!(m[1].label(), "Antonyms");
        assert_eq!(t.look_up("cats").unwrap().0, "Cat");
        assert!(t.look_up("sad").is_none());
    }
}
