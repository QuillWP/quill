use super::{position, round_trip, valid};
use quill_core::character_styles::CharacterFormat;
use quill_core::props::CharProps;
use quill_core::{Document, Position, Rgb, Selection, StoryId};

const MAIN: StoryId = StoryId::Main;
const NAMES: [&str; 5] = ["Emphasis", "Strong", "Code", "Hyperlink", "Label"];

fn runs(doc: &Document) -> impl Iterator<Item = (u16, CharProps)> + '_ {
    [&doc.main, &doc.notes, &doc.annotations]
        .into_iter()
        .chain(doc.headers.iter())
        .flat_map(|story| &story.paras)
        .flat_map(|p| {
            p.runs
                .iter()
                .map(|r| r.chp)
                .chain([p.mark_chp])
                .map(|c| (p.pap.style, c))
        })
}

/// What a named style promises: every field it defines shows on its text
/// unless that text overrides the field by hand.
pub fn check(doc: &Document) {
    for (style, c) in runs(doc) {
        if c.character_style == 0 {
            assert_eq!(c.direct_format, 0, "unstyled text has no overrides");
            continue;
        }
        let definition = doc
            .styles
            .character(c.character_style)
            .expect("text points at a definition")
            .format;
        let base = doc.styles.get(style).chp;
        let mut expected = c;
        CharacterFormat::copy_fields(
            &mut expected,
            definition.resolved(base),
            definition.mask & !c.direct_format,
        );
        assert_eq!(c, expected, "text must show the style it names");
    }
}

fn format(doc: &mut Document, a: u8, b: u8) -> CharacterFormat {
    let mut props = CharProps::default();
    let mut mask = 0;
    if a & 1 != 0 {
        props.bold = true;
        mask |= 1 << 2;
    }
    if a & 2 != 0 {
        props.italic = b & 1 == 0;
        mask |= 1 << 3;
    }
    if a & 4 != 0 {
        props.color = Rgb(0x10_2030 * (u32::from(b) % 8));
        mask |= 1 << 11;
    }
    if a & 8 != 0 {
        props.size = 16 + u16::from(b) % 32;
        mask |= 1 << 1;
    }
    if a & 16 != 0 {
        props.font = doc.font_id(["Georgia", "Courier New", "Missing"][usize::from(b) % 3]);
        mask |= 1;
    }
    CharacterFormat::new(props, mask)
}

fn id(doc: &Document, byte: u8) -> u16 {
    match doc.styles.characters.len() {
        0 => 0,
        n => (usize::from(byte) % n + 1) as u16,
    }
}

/// Bounded programs of everything that touches named character styles. Undo
/// and Redo must restore the exact document, and text must keep following its
/// definition after every step.
pub fn exercise(data: &[u8]) {
    let mut doc = quill_core::text::from_plain_text("First café\nSecond \u{1f980} paragraph");
    doc.headers[0] = quill_core::text::from_plain_text("A header").main;
    let heading = doc.add_style(
        "Heading",
        0,
        None,
        Default::default(),
        CharProps {
            bold: true,
            ..Default::default()
        },
    );
    doc.forget_history();
    let mut undo = Vec::new();
    let mut redo = Vec::new();
    for op in data.as_chunks::<6>().0.iter().take(96) {
        valid(&doc);
        check(&doc);
        let selection = Selection::new(position(&doc, op[1], op[2]), position(&doc, op[3], op[4]));
        let (start, end) = selection.range();
        match op[0] % 16 {
            13 => {
                if let Some(expected) = undo.pop() {
                    let current = round_trip::snapshot(&doc);
                    doc.undo().expect("model has an undo step");
                    assert_eq!(round_trip::snapshot(&doc), expected, "Undo");
                    redo.push(current);
                }
            }
            14 => {
                if let Some(expected) = redo.pop() {
                    let current = round_trip::snapshot(&doc);
                    doc.redo().expect("model has a redo step");
                    assert_eq!(round_trip::snapshot(&doc), expected, "Redo");
                    undo.push(current);
                }
            }
            15 => round_trip::check_supported(&doc),
            action => {
                let before = round_trip::snapshot(&doc);
                doc.mark_saved();
                doc.begin("Fuzz style", selection);
                let chosen = id(&doc, op[5]);
                let name = NAMES[usize::from(op[5]) % NAMES.len()];
                match action {
                    0 => {
                        let format = format(&mut doc, op[3], op[4]);
                        doc.add_character_style(name, format);
                    }
                    1 => doc.apply_character_style(MAIN, start, end, chosen),
                    2 => doc.apply_character_style(MAIN, start, end, 0),
                    3 if chosen != 0 => {
                        let format = format(&mut doc, op[3], op[4]);
                        doc.redefine_character_style(chosen, format);
                    }
                    4 if chosen != 0 => {
                        doc.rename_character_style(chosen, name);
                    }
                    5 if chosen != 0 => {
                        doc.delete_character_style(chosen);
                    }
                    6 => doc.format_chars(MAIN, start, end, |c| c.italic = !c.italic),
                    7 => doc.format_chars(MAIN, start, end, |c| c.size = 20 + op[5] as u16 % 9),
                    8 => {
                        doc.insert_plain(MAIN, start, "new ", CharProps::default());
                    }
                    9 => {
                        doc.delete(MAIN, start, end);
                    }
                    10 => {
                        let style = if op[5] & 1 == 0 { heading } else { 0 };
                        doc.apply_style(MAIN, start.para, end.para, style);
                    }
                    11 => doc.revert_to_style(MAIN, start.para, end.para),
                    12 => {
                        let from = doc.font_id("Missing");
                        let to = doc.font_id("Georgia");
                        doc.replace_font(&[from], to);
                    }
                    _ => {}
                }
                let at = doc.main.clamp(start);
                doc.commit(Selection::caret(at));
                // Replace Fonts changes headers outside Undo, as documented
                // on replace_font, so it ends this model's history.
                if action == 12 {
                    doc.forget_history();
                    undo.clear();
                    redo.clear();
                } else if doc.is_modified() {
                    undo.push(before);
                    redo.clear();
                }
            }
        }
        valid(&doc);
        check(&doc);
    }
    round_trip::check_supported(&doc);
}

/// One style over each story, for the fixed property tests.
#[allow(dead_code)]
pub fn styled(id_name: &str) -> Document {
    let mut doc = quill_core::text::from_plain_text("Styled text");
    let format = CharacterFormat::new(
        CharProps {
            italic: true,
            ..Default::default()
        },
        1 << 3,
    );
    let id = doc.add_character_style(id_name, format).unwrap();
    doc.apply_character_style(MAIN, Position::new(0, 0), Position::new(0, 6), id);
    doc.forget_history();
    doc
}
