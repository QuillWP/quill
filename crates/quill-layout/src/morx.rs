//! Apple's `morx` glyph substitution, as far as emoji sequences need it.
//!
//! Apple Color Emoji has no GSUB table. It keeps flags, joined (ZWJ)
//! sequences, skin tones and the emoji form of a text symbol in `morx`
//! state machines instead. `ttf-parser` reads the table; this runs it.
//! Only the three kinds of subtable that font switches on by default are
//! run: non-contextual, contextual and ligature. Rearrangement and
//! insertion subtables, and subtables for vertical or backwards text, are
//! skipped.

use ttf_parser::{Face, GlyphId, morx::SubtableKind};

/// The glyph ID a state machine uses for a glyph a ligature removed.
const DELETED: u16 = 0xFFFF;

/// Class numbers every state table shares.
const CLASS_END_OF_TEXT: u16 = 0;
const CLASS_OUT_OF_BOUNDS: u16 = 1;

/// An entry's flags in a contextual or ligature subtable.
const SET_MARK: u16 = 0x8000;
const SET_COMPONENT: u16 = 0x8000;
const DONT_ADVANCE: u16 = 0x4000;
const PERFORM_ACTION: u16 = 0x2000;

/// A ligature action's bits.
const ACTION_LAST: u32 = 0x8000_0000;
const ACTION_STORE: u32 = 0x4000_0000;
const ACTION_OFFSET: u32 = 0x3FFF_FFFF;

/// A ligature never gathers more glyphs than this.
const MAX_STACK: usize = 64;

/// One glyph of the result and the first input glyph it stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shaped {
    pub glyph: u16,
    pub first: usize,
}

#[derive(Clone, Copy)]
struct Slot {
    glyph: u16,
    cluster: usize,
}

/// Run `face`'s default `morx` features over `input` (glyph IDs in text
/// order). None if the face has no `morx` table. Glyphs that ligatures
/// absorbed are gone from the result, so the glyph at `first` covers the
/// input up to the next result glyph's `first`.
pub fn shape(face: &Face, input: &[u16]) -> Option<Vec<Shaped>> {
    let table = face.tables().morx.as_ref()?;
    let mut buf: Vec<Slot> = input
        .iter()
        .enumerate()
        .map(|(cluster, &glyph)| Slot { glyph, cluster })
        .collect();
    for chain in table.chains {
        for sub in chain.subtables {
            if sub.feature_flags & chain.default_flags == 0 {
                continue;
            }
            let coverage = sub.coverage;
            if (coverage.is_vertical() && !coverage.is_all_directions()) || coverage.is_backwards()
            {
                continue;
            }
            match &sub.kind {
                SubtableKind::NonContextual(lookup) => {
                    for slot in &mut buf {
                        if let Some(glyph) = lookup.value(GlyphId(slot.glyph)) {
                            slot.glyph = glyph;
                        }
                    }
                }
                SubtableKind::Contextual(sub) => contextual(sub, &mut buf),
                SubtableKind::Ligature(sub) => ligature(sub, &mut buf),
                SubtableKind::Rearrangement(_) | SubtableKind::Insertion(_) => {}
            }
        }
    }
    Some(
        buf.into_iter()
            .filter(|slot| slot.glyph != DELETED)
            .map(|slot| Shaped {
                glyph: slot.glyph,
                first: slot.cluster,
            })
            .collect(),
    )
}

/// Walk a state table over `buf`, running `body` for every entry the table
/// takes. `idx` is the glyph the machine is looking at, or the glyph count
/// at the end of the text. A macro because `ttf-parser` does not let the
/// table's types be named.
macro_rules! drive {
    ($table:expr, $buf:ident, |$idx:ident, $entry:ident| $body:block) => {{
        let table = &$table;
        let mut state = 0;
        let mut position = 0;
        // A table that keeps declining to advance must still end.
        let mut budget = 16 * ($buf.len() + 4);
        loop {
            let class = if position < $buf.len() {
                table
                    .class(GlyphId($buf[position].glyph))
                    .unwrap_or(CLASS_OUT_OF_BOUNDS)
            } else {
                CLASS_END_OF_TEXT
            };
            let Some($entry) = table.entry(state, class) else {
                break;
            };
            let $idx = position;
            $body
            state = $entry.new_state;
            if position >= $buf.len() {
                break;
            }
            budget -= 1;
            if $entry.flags & DONT_ADVANCE == 0 || budget == 0 {
                position += 1;
            }
        }
    }};
}

fn contextual(sub: &ttf_parser::morx::ContextualSubtable, buf: &mut [Slot]) {
    let mut mark: Option<usize> = None;
    let substitute = |slot: &mut Slot, index: u16| {
        if index == 0xFFFF {
            return;
        }
        if let Some(glyph) = sub
            .lookup(u32::from(index))
            .and_then(|lookup| lookup.value(GlyphId(slot.glyph)))
        {
            slot.glyph = glyph;
        }
    };
    drive!(sub.state, buf, |idx, entry| {
        // Nothing to substitute at the end of the text unless a mark is set.
        if idx < buf.len() || mark.is_some() {
            if let Some(m) = mark
                && let Some(slot) = buf.get_mut(m)
            {
                substitute(slot, entry.extra.mark_index);
            }
            if let Some(slot) = buf.get_mut(idx.min(buf.len().saturating_sub(1))) {
                substitute(slot, entry.extra.current_index);
            }
            if entry.flags & SET_MARK != 0 {
                mark = Some(idx);
            }
        }
    });
}

fn ligature(sub: &ttf_parser::morx::LigatureSubtable, buf: &mut [Slot]) {
    // Positions of the glyphs that may join, last pushed last.
    let mut stack: Vec<usize> = Vec::new();
    drive!(sub.state, buf, |idx, entry| {
        if entry.flags & SET_COMPONENT != 0 && idx < buf.len() && stack.len() < MAX_STACK {
            stack.push(idx);
        }
        if entry.flags & PERFORM_ACTION != 0 && !stack.is_empty() {
            let mut action_index = u32::from(entry.extra);
            let mut cursor = stack.len();
            let mut ligature_index: u32 = 0;
            loop {
                if cursor == 0 {
                    // More actions than glyphs: the stack is spent.
                    stack.clear();
                    break;
                }
                cursor -= 1;
                let at = stack[cursor];
                let Some(action) = sub.ligature_actions.get(action_index) else {
                    break;
                };
                let mut offset = action & ACTION_OFFSET;
                if offset & 0x2000_0000 != 0 {
                    offset |= 0xC000_0000;
                }
                let component = i64::from(buf[at].glyph) + i64::from(offset as i32);
                let Some(part) = u32::try_from(component)
                    .ok()
                    .and_then(|i| sub.components.get(i))
                else {
                    break;
                };
                ligature_index += u32::from(part);
                if action & (ACTION_STORE | ACTION_LAST) != 0 {
                    let Some(glyph) = sub.ligatures.get(ligature_index) else {
                        break;
                    };
                    buf[at].glyph = glyph.0;
                    let end = stack.last().map_or(at, |&last| last) + 1;
                    // The glyphs after this one were its components: they go.
                    while stack.len() > cursor + 1 {
                        if let Some(gone) = stack.pop() {
                            buf[gone].glyph = DELETED;
                        }
                    }
                    let cluster = buf[at..end].iter().map(|s| s.cluster).min().unwrap_or(0);
                    for slot in &mut buf[at..end] {
                        slot.cluster = cluster;
                    }
                }
                action_index += 1;
                if action & ACTION_LAST != 0 {
                    break;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many glyphs Apple Color Emoji makes of `chars`, or None where the
    /// font is not installed.
    fn glyph_count(chars: &str) -> Option<usize> {
        let data = std::fs::read("/System/Library/Fonts/Apple Color Emoji.ttc").ok()?;
        let face = Face::parse(&data, 0).ok()?;
        let ids: Vec<u16> = chars
            .chars()
            .map(|c| face.glyph_index(c).map_or(0, |g| g.0))
            .collect();
        assert!(!ids.contains(&0), "the font lacks a character of {chars:?}");
        shape(&face, &ids).map(|shaped| shaped.len())
    }

    #[test]
    fn sequences_the_font_defines_become_one_glyph() {
        // CoreText makes one glyph of each of these.
        for chars in [
            "\u{1F1EB}\u{1F1F7}",
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
            "\u{1F469}\u{1F3FD}\u{200D}\u{2695}\u{FE0F}",
            "\u{1F44D}\u{1F3FD}",
            "\u{2764}\u{FE0F}",
            "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}",
            "1\u{FE0F}\u{20E3}",
            "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}",
        ] {
            if let Some(n) = glyph_count(chars) {
                assert_eq!(n, 1, "{chars:?}");
            }
        }
    }

    #[test]
    fn characters_that_do_not_combine_stay_separate() {
        // Two faces side by side, or a lone regional indicator after a pair.
        for (chars, glyphs) in [
            ("\u{1F600}\u{1F600}", 2),
            ("\u{1F1EB}\u{1F1F7}\u{1F1E9}", 2),
        ] {
            if let Some(n) = glyph_count(chars) {
                assert_eq!(n, glyphs, "{chars:?}");
            }
        }
    }
}
