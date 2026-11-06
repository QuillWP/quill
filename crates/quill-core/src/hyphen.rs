//! Hyphenation by Liang's patterns (TeX's method): every pattern that
//! matches part of a word votes on the gaps between its letters, odd
//! numbers for a break and even ones against, and the highest vote wins.

use std::collections::HashMap;

pub struct Hyphenator {
    /// Letters of a pattern -> the votes on the gaps around them.
    patterns: HashMap<String, Vec<u8>>,
    longest: usize,
    /// Letters a word must keep before its first break and after its last.
    left: usize,
    right: usize,
}

impl Hyphenator {
    /// Patterns one per line ("a2ch4", ".ad4der"), the first line holding
    /// the shortest left and right parts ("2 3").
    pub fn from_text(text: &str) -> Self {
        let mut lines = text.lines();
        let mins: Vec<usize> = lines
            .next()
            .unwrap_or("")
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        let mut patterns = HashMap::new();
        let mut longest = 0;
        for line in lines {
            let p = line.trim();
            if p.is_empty() {
                continue;
            }
            let mut letters = String::new();
            let mut votes = vec![0u8];
            for c in p.chars() {
                match c.to_digit(10) {
                    Some(d) => *votes.last_mut().unwrap() = d as u8,
                    None => {
                        letters.push(c);
                        votes.push(0);
                    }
                }
            }
            longest = longest.max(letters.chars().count());
            patterns.insert(letters, votes);
        }
        Hyphenator {
            patterns,
            longest,
            left: mins.first().copied().unwrap_or(2),
            right: mins.get(1).copied().unwrap_or(3),
        }
    }

    /// Where `word` may break: character counts from its start.
    pub fn breaks(&self, word: &str) -> Vec<usize> {
        let lower: Vec<char> = word.to_lowercase().chars().collect();
        let n = lower.len();
        if n < self.left + self.right || lower.iter().any(|c| !c.is_alphabetic()) {
            return Vec::new();
        }
        let dotted: Vec<char> = std::iter::once('.')
            .chain(lower.iter().copied())
            .chain(std::iter::once('.'))
            .collect();
        let mut votes = vec![0u8; dotted.len() + 1];
        for start in 0..dotted.len() {
            let mut key = String::new();
            for &ch in dotted[start..].iter().take(self.longest) {
                key.push(ch);
                if let Some(v) = self.patterns.get(&key) {
                    for (k, &d) in v.iter().enumerate() {
                        let slot = &mut votes[start + k];
                        *slot = (*slot).max(d);
                    }
                }
            }
        }
        // votes[i + 1] is the gap before letter i of the word.
        (self.left..=n - self.right)
            .filter(|&i| votes[i + 1] % 2 == 1)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liang_patterns_vote() {
        // Knuth's example patterns for "hyphenation".
        let h =
            Hyphenator::from_text("2 3\nhy3ph\nhe2n\nhena4\nhen5at\n1na\nn2at\n1tio\n2io\no2n\n");
        let w = "hyphenation";
        let parts: Vec<usize> = h.breaks(w);
        assert_eq!(parts, [2, 6]);
        assert!(h.breaks("ab").is_empty());
    }
}
