//! Screen-only proofing. Native dictionary calls run in small idle batches,
//! never from typing or painting; edits invalidate ranges before repainting.

use super::*;
use std::collections::HashMap;

const DELAY: Duration = Duration::from_millis(400);
const SLICE: Duration = Duration::from_millis(4);
const INTERVAL: Duration = Duration::from_millis(40);
const MAX_PARAS: usize = 128;
const MAX_WORDS: usize = 4096;
const MAX_BAD: usize = 4096;
const MAX_WORD_BYTES: usize = 256;
const RED: u32 = 0xD93025;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Config {
    enabled: bool,
    upper: bool,
    numbers: bool,
    revision: u64,
}

impl Config {
    fn current() -> Self {
        crate::settings::with(|s| Self {
            enabled: s.spelling_as_you_type,
            upper: s.ignore_uppercase,
            numbers: s.ignore_numbers,
            revision: crate::language::spelling_revision(),
        })
    }
}

#[derive(Default)]
struct Scan {
    cursor: usize,
    start: Option<usize>,
    run: usize,
    run_end: usize,
    word: String,
    too_long: bool,
    bad: Vec<(usize, usize)>,
    complete: bool,
}

pub(super) struct LiveSpelling {
    config: Option<Config>,
    paragraphs: HashMap<usize, Scan>,
    words: HashMap<String, bool>,
    due: Instant,
}

impl Default for LiveSpelling {
    fn default() -> Self {
        Self {
            config: None,
            paragraphs: HashMap::new(),
            words: HashMap::new(),
            due: Instant::now() + DELAY,
        }
    }
}

fn visible(chp: CharProps) -> bool {
    !chp.hidden && !quill_core::revision::is_deleted(chp.rev)
}

/// Resume a word scan without allocating a paragraph-sized word/character list.
/// Deleted letters/separators are skipped, so the accepted spelling is checked.
fn next_word(p: &Paragraph, scan: &mut Scan, budget: &mut usize) -> Option<(usize, usize, String)> {
    while scan.cursor < p.len() && *budget > 0 {
        let i = scan.cursor;
        let ch = p.text[i..].chars().next()?;
        scan.cursor += ch.len_utf8();
        *budget -= 1;
        // Advance through formatting runs once, rather than searching every
        // run again for each character in a heavily formatted paragraph.
        if scan.run_end == 0 {
            scan.run_end = p.runs.first().map_or(p.len(), |r| r.len);
        }
        while i >= scan.run_end && scan.run + 1 < p.runs.len() {
            scan.run += 1;
            scan.run_end += p.runs[scan.run].len;
        }
        let chp = p.runs.get(scan.run).map_or(p.mark_chp, |r| r.chp);
        if !visible(chp) {
            continue;
        }
        let apostrophe = matches!(ch, '\'' | '\u{2019}')
            && scan.start.is_some()
            && p.text[scan.cursor..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric);
        if ch.is_alphanumeric() || apostrophe {
            scan.start.get_or_insert(i);
            if !scan.too_long {
                if scan.word.len() + ch.len_utf8() <= MAX_WORD_BYTES {
                    scan.word.push(ch);
                } else {
                    scan.too_long = true;
                    scan.word.clear();
                }
            }
        } else if let Some(start) = scan.start.take() {
            scan.too_long = false;
            return Some((start, i, std::mem::take(&mut scan.word)));
        }
    }
    if scan.cursor == p.len() {
        scan.complete = true;
        scan.too_long = false;
        return scan
            .start
            .take()
            .map(|s| (s, p.len(), std::mem::take(&mut scan.word)));
    }
    None
}

impl LiveSpelling {
    pub(super) fn invalidate(&mut self) {
        self.paragraphs.clear();
        self.due = Instant::now() + DELAY;
    }

    fn needs_work(&self, config: Config, visible: &[usize]) -> bool {
        config.enabled
            && (self.config != Some(config)
                || visible
                    .iter()
                    .any(|i| self.paragraphs.get(i).is_none_or(|p| !p.complete)))
    }

    fn tick(
        &mut self,
        paras: &[Paragraph],
        visible_paras: &[usize],
        config: Config,
        now: Instant,
        mut check: impl FnMut(&str) -> bool,
    ) -> bool {
        let mut changed = false;
        if self.config != Some(config) {
            changed = self.paragraphs.values().any(|p| !p.bad.is_empty());
            self.config = Some(config);
            self.paragraphs.clear();
            self.words.clear();
        }
        if !config.enabled || now < self.due {
            return changed;
        }
        self.due = now + INTERVAL;
        self.paragraphs.retain(|i, _| visible_paras.contains(i));
        let started = Instant::now();
        let (mut chars, mut calls) = (2048, 8);
        for &i in visible_paras.iter().take(MAX_PARAS) {
            let Some(p) = paras.get(i) else { continue };
            let scan = self.paragraphs.entry(i).or_default();
            while !scan.complete && chars > 0 && calls > 0 && started.elapsed() < SLICE {
                let Some((s, e, word)) = next_word(p, scan, &mut chars) else {
                    break;
                };
                if scan.bad.len() >= MAX_BAD {
                    continue;
                }
                if word.is_empty()
                    || (config.numbers && word.chars().any(char::is_numeric))
                    || (config.upper
                        && word.chars().count() > 1
                        && word.chars().all(|c| !c.is_lowercase()))
                {
                    continue;
                }
                let correct = if let Some(&ok) = self.words.get(&word) {
                    ok
                } else {
                    calls -= 1;
                    let ok = check(&word);
                    if self.words.len() >= MAX_WORDS {
                        self.words.clear();
                    }
                    self.words.insert(word, ok);
                    ok
                };
                if !correct {
                    scan.bad.push((s, e));
                    changed = true;
                }
            }
            if chars == 0 || calls == 0 || started.elapsed() >= SLICE {
                break;
            }
        }
        changed
    }
}

impl DocView {
    fn spelling_paragraphs(&self) -> Vec<usize> {
        let mut out = Vec::new();
        // The newest drawn ranges cover the current viewport and split pane.
        for &(a, b) in &self.drawn {
            for i in a..b.min(self.doc.main.paras.len()) {
                if !out.contains(&i) {
                    out.push(i);
                }
                if out.len() == MAX_PARAS {
                    return out;
                }
            }
        }
        if out.is_empty() && self.sel.focus.para < self.doc.main.paras.len() {
            out.push(self.sel.focus.para);
        }
        out
    }

    pub(crate) fn live_spelling_deadline(&self) -> Option<Instant> {
        if self.printing || self.is_composing() {
            return None;
        }
        self.spelling
            .needs_work(Config::current(), &self.spelling_paragraphs())
            .then(|| self.spelling.due.max(Instant::now() + INTERVAL))
    }

    pub(crate) fn check_live_spelling(&mut self) -> bool {
        if self.printing || self.is_composing() {
            return false;
        }
        let visible = self.spelling_paragraphs();
        self.spelling.tick(
            &self.doc.main.paras,
            &visible,
            Config::current(),
            Instant::now(),
            crate::language::word_ok,
        )
    }

    pub(super) fn draw_spelling(
        &self,
        c: &mut dyn Surface,
        para: usize,
        l: &ParaLayout,
        ox: f32,
        py: f32,
    ) {
        if !c.screen() || self.printing || !crate::settings::with(|s| s.spelling_as_you_type) {
            return;
        }
        if self.spelling.config != Some(Config::current()) {
            return;
        }
        let Some(scan) = self.spelling.paragraphs.get(&para) else {
            return;
        };
        for line in &l.lines {
            // Page Layout clips at the last line's bottom. Keep the complete
            // wave inside that line, including for fonts with shallow descents.
            let underline_y =
                py + line.top + (line.baseline + 2.0).min((line.height - 2.5).max(0.0));
            let mut segment: Option<(usize, f32, f32)> = None;
            for g in l.line_glyphs(line) {
                let off = g.offset as usize;
                let k = scan.bad.partition_point(|&(_, end)| end <= off);
                let bad = scan
                    .bad
                    .get(k)
                    .filter(|&&(s, e)| s <= off && off < e)
                    .filter(|&&(s, e)| {
                        !(self.sel.is_empty()
                            && self.sel.focus.para == para
                            && (s..=e).contains(&self.sel.focus.offset))
                    })
                    .is_some();
                if bad
                    && matches!(g.kind, Kind::Text)
                    && g.advance > 0.0
                    && visible(l.styles[g.style as usize].chp)
                {
                    if let Some((prev, _, _)) = segment
                        && prev != k
                    {
                        let (_, a, b) = segment.take().unwrap();
                        wave(c, ox + a, ox + b, underline_y);
                    }
                    match &mut segment {
                        Some((_, _, b)) => *b = g.x + g.advance,
                        None => segment = Some((k, g.x, g.x + g.advance)),
                    }
                } else if let Some((_, a, b)) = segment.take() {
                    wave(c, ox + a, ox + b, underline_y);
                }
            }
            if let Some((_, a, b)) = segment {
                wave(c, ox + a, ox + b, underline_y);
            }
        }
    }
}

fn wave(c: &mut dyn Surface, from: f32, to: f32, y: f32) {
    let mut x = from;
    let mut step = 0;
    while x < to {
        let dy = [0.0, 0.75, 1.5, 0.75][step % 4];
        c.fill(
            Rect::new(x, y + dy, (to - x).min(1.0), 0.85_f32.max(c.pixel())),
            RED,
        );
        x += 1.0;
        step += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::text::from_plain_text;

    fn config() -> Config {
        Config {
            enabled: true,
            upper: true,
            numbers: true,
            revision: 0,
        }
    }

    fn finish(
        state: &mut LiveSpelling,
        p: &[Paragraph],
        config: Config,
        mut check: impl FnMut(&str) -> bool,
    ) {
        for _ in 0..200 {
            let now = state.due;
            state.tick(p, &[0], config, now, &mut check);
            if !state.needs_work(config, &[0]) {
                return;
            }
        }
        panic!("bounded spelling scan did not finish");
    }

    #[test]
    fn live_spelling_limits_native_calls_reuses_words_and_resumes_long_paragraphs() {
        let doc = from_plain_text(&format!("{} ", "misspelled ".repeat(300)));
        let mut state = LiveSpelling::default();
        let mut calls = 0;
        let now = state.due;
        state.tick(&doc.main.paras, &[0], config(), now, |_| {
            calls += 1;
            false
        });
        assert!(
            !state.paragraphs[&0].complete,
            "character budget must stop this scan"
        );
        finish(&mut state, &doc.main.paras, config(), |_| {
            calls += 1;
            false
        });
        assert_eq!(
            calls, 1,
            "paint and repeated words need no further native calls"
        );
        assert_eq!(state.paragraphs[&0].bad.len(), 300);
        let distinct =
            from_plain_text("one two three four five six seven eight nine ten eleven twelve");
        state.invalidate();
        let mut calls = 0;
        let now = state.due;
        state.tick(&distinct.main.paras, &[0], config(), now, |_| {
            calls += 1;
            false
        });
        assert!(calls <= 8);
        assert!(!state.paragraphs[&0].complete);
        finish(&mut state, &distinct.main.paras, config(), |_| false);
        assert_eq!(state.paragraphs[&0].bad.len(), 12);
    }

    #[test]
    fn live_spelling_handles_unicode_hidden_deleted_and_ignore_preferences() {
        let mut doc = from_plain_text("café don't NASA abc12 secret deleted");
        let hidden = doc.main.paras[0].text.find("secret").unwrap();
        doc.format_chars(
            MAIN,
            Position::new(0, hidden),
            Position::new(0, hidden + 6),
            |c| c.hidden = true,
        );
        let rev = quill_core::revision::intern(quill_core::revision::Revision {
            deleted: Some(quill_core::revision::Stamp::new(
                "Editor",
                "2026-10-26T12:00:00Z",
            )),
            ..Default::default()
        });
        let deleted = doc.main.paras[0].text.find("deleted").unwrap();
        doc.format_chars(
            MAIN,
            Position::new(0, deleted),
            Position::new(0, deleted + 7),
            |c| c.rev = rev,
        );
        let mut state = LiveSpelling::default();
        let mut seen = Vec::new();
        finish(&mut state, &doc.main.paras, config(), |w| {
            seen.push(w.to_string());
            false
        });
        assert_eq!(seen, ["café", "don't"]);
        assert_eq!(state.paragraphs[&0].bad, [(0, 5), (6, 11)]);
        let mut cfg = config();
        cfg.upper = false;
        cfg.numbers = false;
        seen.clear();
        finish(&mut state, &doc.main.paras, cfg, |w| {
            seen.push(w.to_string());
            true
        });
        assert_eq!(seen, ["café", "don't", "NASA", "abc12"]);
        assert!(state.paragraphs[&0].bad.is_empty());
    }

    #[test]
    fn live_spelling_edits_and_dictionary_changes_remove_stale_ranges() {
        let mut doc = from_plain_text("mistkae ");
        let mut state = LiveSpelling::default();
        finish(&mut state, &doc.main.paras, config(), |_| false);
        assert_eq!(state.paragraphs[&0].bad, [(0, 7)]);
        let mut cfg = config();
        cfg.revision += 1;
        finish(&mut state, &doc.main.paras, cfg, |_| true);
        assert!(
            state.paragraphs[&0].bad.is_empty(),
            "Add/Ignore invalidates cached verdicts"
        );
        doc.main.paras[0].text = "correct ".into();
        state.invalidate();
        assert!(
            state.paragraphs.is_empty(),
            "old offsets disappear immediately on edit"
        );
        assert!(state.due > Instant::now(), "typing defers native queries");
        finish(&mut state, &doc.main.paras, cfg, |_| true);
        assert!(state.paragraphs[&0].bad.is_empty());
    }

    #[test]
    fn live_spelling_edit_and_undo_invalidate_view_ranges() {
        let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
        let mut view = DocView::new(from_plain_text("mispelled "), "Edit".into());
        view.update_layout(&mut fonts);
        finish(
            &mut view.spelling,
            &view.doc.main.paras,
            Config::current(),
            |_| false,
        );
        assert!(!view.spelling.paragraphs[&0].bad.is_empty());
        view.sel = Selection::new(Position::new(0, 0), Position::new(0, 9));
        view.type_text("correct");
        view.update_layout(&mut fonts);
        assert!(view.spelling.paragraphs.is_empty());
        finish(
            &mut view.spelling,
            &view.doc.main.paras,
            Config::current(),
            |_| true,
        );
        assert!(view.spelling.paragraphs[&0].bad.is_empty());
        view.exec(crate::commands::Cmd::Undo, false, &mut Clipboard::new());
        view.update_layout(&mut fonts);
        assert_eq!(view.doc.main.paras[0].text, "mispelled ");
        assert!(view.spelling.paragraphs.is_empty());
    }

    #[test]
    fn live_spelling_long_tokens_are_bounded_and_do_not_hide_later_words() {
        let doc = from_plain_text(&format!("{} typo ", "a".repeat(12000)));
        let mut state = LiveSpelling::default();
        let mut checked = Vec::new();
        finish(&mut state, &doc.main.paras, config(), |word| {
            checked.push(word.to_owned());
            false
        });
        assert_eq!(checked, ["typo"]);
        assert_eq!(state.paragraphs[&0].bad, [(12001, 12005)]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn live_spelling_mac_service_marks_a_real_misspelling() {
        assert!(
            crate::spellcheck::available(),
            "Mac spelling service must be available for this host check"
        );
        assert_eq!(crate::spellcheck::check("zzqvxqword"), Some(false));
        let doc = from_plain_text("zzqvxqword ");
        let mut state = LiveSpelling::default();
        finish(
            &mut state,
            &doc.main.paras,
            config(),
            crate::language::word_ok,
        );
        assert_eq!(state.paragraphs[&0].bad, [(0, 10)]);
    }

    #[test]
    fn live_spelling_default_and_opt_out_persist() {
        let mut settings = crate::settings::Settings::default();
        assert!(settings.spelling_as_you_type);
        assert!(crate::settings::Settings::from_text("name = Ada\n").spelling_as_you_type);
        settings.spelling_as_you_type = false;
        assert!(!crate::settings::Settings::from_text(&settings.to_text()).spelling_as_you_type);
    }

    #[test]
    fn live_spelling_red_squiggles_follow_wrapped_text_and_stay_out_of_pdf() {
        let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
        assert!(!fonts.family_names().is_empty());
        let mut doc = from_plain_text("mispelled sentense mispelled sentense ");
        doc.props.page_width = 3600;
        doc.props.margin_left = 720;
        doc.props.margin_right = 720;
        let mut view = DocView::new(doc, "Spelling".into());
        view.sel = Selection::caret(Position::new(0, view.doc.main.paras[0].len()));
        view.area = Rect::new(0.0, 0.0, 400.0, 300.0);
        view.update_layout(&mut fonts);
        let cfg = Config::current();
        finish(&mut view.spelling, &view.doc.main.paras, cfg, |_| false);
        assert!(view.layout(0).lines.len() > 1);
        let original = view.doc.main.paras.clone();
        let modified = view.doc.is_modified();
        let mut pixels = vec![0u32; 400 * 300];
        view.draw(
            &mut Canvas::new(&mut pixels, 400, 300, 1.0),
            &fonts,
            &mut GlyphCache::default(),
            true,
        );
        assert!(
            pixels.contains(&RED),
            "actual screen raster must contain red squiggles"
        );
        view.page_view = true;
        view.update_layout(&mut fonts);
        pixels.fill(0);
        view.draw(
            &mut Canvas::new(&mut pixels, 400, 300, 1.0),
            &fonts,
            &mut GlyphCache::default(),
            true,
        );
        assert!(
            pixels.contains(&RED),
            "Page Layout must not clip away the proofing marks"
        );
        let pages = view.pages.len();
        let with_spelling =
            crate::pdf::write(&mut view, &mut fonts, &mut GlyphCache::default(), 0..pages);
        view.spelling.paragraphs.clear();
        let without_spelling =
            crate::pdf::write(&mut view, &mut fonts, &mut GlyphCache::default(), 0..pages);
        assert_eq!(
            with_spelling, without_spelling,
            "proofing marks must never reach print/PDF"
        );
        assert_eq!(view.doc.main.paras, original);
        assert_eq!(view.doc.is_modified(), modified);
    }

    #[test]
    fn live_spelling_suppresses_the_word_at_the_caret() {
        let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
        let mut view = DocView::new(from_plain_text("mispelled "), "Caret".into());
        view.area = Rect::new(0.0, 0.0, 300.0, 100.0);
        view.update_layout(&mut fonts);
        finish(
            &mut view.spelling,
            &view.doc.main.paras,
            Config::current(),
            |_| false,
        );
        let mut pixels = vec![0u32; 300 * 100];
        let l = view.layout(0);
        view.draw_spelling(
            &mut Canvas::new(&mut pixels, 300, 100, 1.0),
            0,
            l,
            10.0,
            10.0,
        );
        assert!(!pixels.contains(&RED));
        view.sel = Selection::caret(Position::new(0, 10));
        view.draw_spelling(
            &mut Canvas::new(&mut pixels, 300, 100, 1.0),
            0,
            view.layout(0),
            10.0,
            10.0,
        );
        assert!(pixels.contains(&RED));
    }
}
