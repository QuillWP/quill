//! The word-boundary handling shared by spelling, thesaurus and word tools.

/// Where the words are in a paragraph's text, from `from`: byte ranges of
/// runs of letters (with apostrophes inside them).
pub fn words(text: &str, from: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let is_word = |c: char| c.is_alphanumeric();
    let chars: Vec<(usize, char)> = text[from..]
        .char_indices()
        .map(|(i, c)| (i + from, c))
        .collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        let inner_apostrophe = matches!(c, '\'' | '\u{2019}')
            && start.is_some()
            && chars.get(k + 1).is_some_and(|&(_, n)| is_word(n));
        if is_word(c) || inner_apostrophe {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            out.push((s, i));
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_keep_inner_apostrophes() {
        let t = "Don't stop, it's 'quoted' text.";
        let w: Vec<&str> = words(t, 0).into_iter().map(|(s, e)| &t[s..e]).collect();
        assert_eq!(w, ["Don't", "stop", "it's", "quoted", "text"]);
    }
}
